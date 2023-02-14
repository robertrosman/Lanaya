use crate::utils::dirs::app_data_dir;
use crate::utils::string_util;
use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use std::fs::File;
use std::path::Path;

#[derive(serde::Serialize, serde::Deserialize, Debug, Default, PartialEq)]
pub struct Record {
    pub id: u64,
    pub content: String,
    // data_type(文本=text、图片=image)
    pub data_type: String,
    pub md5: String,
    pub create_time: u64,
    pub is_favorite: bool,
    // 仅在搜索返回时使用
    pub content_highlight: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct QueryReq {
    pub key: Option<String>,
    pub limit: Option<usize>,
    pub is_favorite: Option<bool>,
}

pub struct SqliteDB {
    conn: Connection,
}

const SQLITE_FILE: &str = "data.sqlite";

#[allow(unused)]
impl SqliteDB {
    pub fn new() -> Self {
        let data_dir = app_data_dir().unwrap().join(SQLITE_FILE);
        let c = Connection::open_with_flags(data_dir, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
        SqliteDB { conn: c }
    }

    pub fn init() {
        let data_dir = app_data_dir().unwrap().join(SQLITE_FILE);
        if !Path::new(&data_dir).exists() {
            File::create(&data_dir).unwrap();
        }
        let c = Connection::open_with_flags(data_dir, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
        let sql = r#"
        create table if not exists record
        (
            id          INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            content     TEXT,
            data_type   VARCHAR(20) DEFAULT '',
            md5         VARCHAR(200) DEFAULT '',
            create_time INTEGER,
            is_favorite INTEGER DEFAULT 0
        );
        "#;
        c.execute(sql, ()).unwrap();
    }

    pub fn insert_record(&self, r: Record) -> Result<i64> {
        let sql = "insert into record (content,md5,create_time,is_favorite,data_type) values (?1,?2,?3,?4,?5)";
        let md5 = string_util::md5(r.content.as_str());
        let now = chrono::Local::now().timestamp_millis() as u64;
        let res = self
            .conn
            .execute(sql, (&r.content, md5, now, &r.is_favorite, &r.data_type))?;
        Ok(self.conn.last_insert_rowid())
    }

    fn find_record_by_md5(&self, md5: String) -> Result<Record> {
        let sql = "SELECT id, content, md5, create_time, is_favorite FROM record WHERE md5 = ?1";
        let r = self.conn.query_row(sql, [md5], |row| {
            Ok(Record {
                id: row.get(0)?,
                ..Default::default()
            })
        })?;
        Ok(r)
    }

    // 更新时间
    fn update_record_create_time(&self, r: Record) -> Result<()> {
        let sql = "update record set create_time = ?2 where id = ?1";
        // 获取当前毫秒级时间戳
        let now = chrono::Local::now().timestamp_millis() as u64;
        self.conn.execute(sql, [&r.id, &now])?;
        Ok(())
    }

    pub fn insert_if_not_exist(&self, r: Record) -> Result<()> {
        let md5 = string_util::md5(r.content.as_str());
        match self.find_record_by_md5(md5) {
            Ok(res) => {
                self.update_record_create_time(res)?;
            }
            Err(_e) => {
                self.insert_record(r)?;
            }
        }
        Ok(())
    }

    pub fn md5_is_exist(&self, md5: String) -> Result<bool> {
        let sql = "SELECT count(*) FROM record WHERE md5 = ?1";
        let count: u32 = self.conn.query_row(sql, [md5], |row| row.get(0))?;
        Ok(count > 0)
    }

    // 清除数据
    pub fn clear_data(&self) -> Result<()> {
        let sql = "delete from record";
        self.conn.execute(sql, ())?;
        Ok(())
    }

    // 标记为收藏,如有已经收藏了的则取消收藏
    pub fn mark_favorite(&self, id: u64) -> Result<()> {
        let record = self.find_by_id(id)?;
        let sql = "update record set is_favorite = ?2 where id = ?1";
        let is_favorite = if record.is_favorite { 0 } else { 1 };
        self.conn.execute(sql, [&id, &is_favorite])?;
        Ok(())
    }

    pub fn find_all(&self) -> Result<Vec<Record>> {
        let sql = "SELECT id, content, data_type, md5, create_time, is_favorite FROM record order by create_time desc";
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query([])?;
        let mut res = vec![];
        while let Some(row) = rows.next()? {
            let r = Record {
                id: row.get(0)?,
                content: row.get(1)?,
                data_type: row.get(2)?,
                md5: row.get(3)?,
                create_time: row.get(4)?,
                is_favorite: row.get(5)?,
                content_highlight: None,
            };
            res.push(r);
        }
        Ok(res)
    }

    pub fn find_by_key(&self, req: QueryReq) -> Result<Vec<Record>> {
        let mut sql: String = String::new();
        sql.push_str(
            "SELECT id, content, md5, create_time, is_favorite FROM record where data_type='text'",
        );
        let mut limit: usize = 300;
        let mut params: Vec<String> = vec![];
        if let Some(l) = req.limit {
            limit = l;
        }
        params.push(limit.to_string());
        if let Some(k) = &req.key {
            params.push(format!("%{}%", k));
            sql.push_str(format!(" and content like ?{}", params.len()).as_str());
        }
        if let Some(is_fav) = req.is_favorite {
            let is_fav_int = if is_fav { 1 } else { 0 };
            params.push(is_fav_int.to_string());
            sql.push_str(format!(" and is_favorite = ?{}", params.len()).as_str());
        }
        let sql = format!("{} order by create_time desc limit ?1", sql);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        let mut res = vec![];
        while let Some(row) = rows.next()? {
            let content: String = row.get(1)?;
            let content_highlight = match &req.key {
                Some(key) => Some(string_util::highlight(key.as_str(), content.as_str())),
                None => None,
            };
            let r = Record {
                id: row.get(0)?,
                content,
                data_type: "text".to_string(),
                md5: row.get(2)?,
                create_time: row.get(3)?,
                is_favorite: row.get(4)?,
                content_highlight,
            };
            res.push(r);
        }
        Ok(res)
    }

    //删除超过limit的记录
    pub fn delete_over_limit(&self, limit: usize) -> Result<()> {
        // 先查询count，如果count - limit > 50 就删除 超出limit部分记录 主要是防止频繁重建数据库
        let stmt = self.conn.prepare("SELECT count(*) FROM record")?;
        let count = stmt.column_count();
        if count < 50 + limit {
            return Ok(());
        }
        let sql = "DELETE FROM record WHERE id IN (SELECT id FROM record ORDER BY id DESC LIMIT ?1, 1000000000)";
        self.conn.execute(sql, [&limit])?;
        Ok(())
    }

    pub fn find_by_id(&self, id: u64) -> Result<Record> {
        let sql = "SELECT id, content, data_type, md5, create_time, is_favorite FROM record where id = ?1";
        let r = self.conn.query_row(sql, [&id], |row| {
            Ok(Record {
                id: row.get(0)?,
                content: row.get(1)?,
                data_type: row.get(2)?,
                md5: row.get(3)?,
                create_time: row.get(4)?,
                is_favorite: row.get(5)?,
                content_highlight: None,
            })
        })?;
        Ok(r)
    }
}

#[test]
fn test_sqlite_insert() {
    SqliteDB::init();
    let r = Record {
        content: "123456".to_string(),
        md5: "e10adc3949ba59abbe56e057f20f883e".to_string(),
        create_time: 1234568,
        ..Default::default()
    };
    assert_eq!(SqliteDB::new().insert_record(r).unwrap(), 1_i64)
}
