import { createApp } from "vue";
import "./style.css";
import App from "./App.vue";
import router from "./router/router.js";
import { listenLanguageChange } from "./service/globalListener";
import { i18n, setLanguage } from "./i18n";
const app = createApp(App);
app.use(router);
app.use(i18n);
app.mount("#app");
listenLanguageChange((data) => {
  setLanguage(data);
});
