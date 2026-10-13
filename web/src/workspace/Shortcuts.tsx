import { Modal, ModalHeader } from "../ui/overlay";
import { keys } from "./shortcut-keys";

export const SHORTCUTS: { group: string; rows: [string, string][] }[] = [
  { group: "前往", rows: [
    ["Mod+K", "搜尋與指令（Bot、群組、訊息、檔案、定時任務）"],
    ["Mod+Shift+F", "搜尋側欄的 Bot"],
    ["Mod+F", "在這個對話中尋找"],
    ["Alt+↑ / Alt+↓", "上一個／下一個 Bot 或群組"],
    ["Alt+1 … Alt+9", "側欄第 1 到第 9 個 Bot"],
  ] },
  { group: "動作", rows: [
    ["Mod+Alt+N", "新增 Bot 或群組對話"],
    ["Mod+I", "游標移到輸入框"],
    ["Mod+D", "語音輸入"],
    ["Mod+B", "精簡側欄"],
    ["Mod+Shift+,", "目前 Bot 的設定"],
    ["Mod+,", "設定"],
    ["Mod+/", "鍵盤快速鍵"],
  ] },
  { group: "輸入框", rows: [
    ["Enter", "送出"],
    ["Shift+Enter", "換行"],
    ["/", "使用技能"],
    ["@", "提到其他 Bot、群組或應用程式"],
    ["Esc", "關閉選單或對話框"],
  ] },
];

export function ShortcutsDialog({ onClose }: { onClose: () => void }) {
  return (
    <Modal label="鍵盤快速鍵" onClose={onClose} className="shortcuts-dialog">
      <ModalHeader title="鍵盤快速鍵" onClose={onClose} />
      {SHORTCUTS.map(section => (
        <section key={section.group} className="shortcut-group">
          <h3>{section.group}</h3>
          <dl>
            {section.rows.map(([combo, label]) => (
              <div key={combo}>
                <dt>{keys(combo).split(" ").map((part, i) => part === "/" && i > 0 ? <span key={i}> / </span> : part === "…" ? <span key={i}> … </span> : <kbd key={i}>{part}</kbd>)}</dt>
                <dd>{label}</dd>
              </div>
            ))}
          </dl>
        </section>
      ))}
    </Modal>
  );
}
