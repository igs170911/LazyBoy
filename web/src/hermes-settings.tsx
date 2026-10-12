import { type AppSettings } from "./api";

export function HermesSettings({settings}: {settings?: AppSettings}) {
  return <section className="lb-settings-group"><h3>執行核心</h3>
    <p>從這個網頁交辦工作；每個 Bot 在自己的持久電腦裡使用瀏覽器、檔案、記憶與技能。</p>
    <p>可以在工作中補充、停止，或打開電腦直接接手。完成的結果和檔案會留在同一段對話。</p>
    {settings ? <p>預設模型：{settings.active} / {settings.model || "尚未設定"}<br />核心版本：<code>{settings.hermes_commit?.slice(0,12) || "讀取中"}</code></p> : null}
    <p>Bot 指定的模型會優先於預設模型。例行工作使用本產品的同一套排程。</p>
  </section>;
}
