import { t } from "./i18n";

export type CronFreq = "Every hour" | "Every day" | "Weekdays" | "Every week" | "Every month" | "Interval" | "Advanced";
export type CronUnit = "minutes" | "hours" | "days";
export type CronPreset = { freq: CronFreq; n: number; unit: CronUnit; time: string; cron: string };

export interface ScheduleItem {
  id: string;
  botId: string;
  threadId?: string | null;
  name: string;
  cron: string;
  timezone: string;
  instructions: string;
  enabled: boolean;
  human: string;
  lastRunAt?: string | null;
  nextRunAt?: string | null;
}

export const CRON_FREQS: CronFreq[] = ["Every hour", "Every day", "Weekdays", "Every week", "Every month", "Interval", "Advanced"];
const TIMES = ["6:00 AM", "7:00 AM", "8:00 AM", "9:00 AM", "12:00 PM", "3:00 PM", "6:00 PM", "9:00 PM"];
const NUMBERS = [1, 2, 3, 5, 10, 15, 30, 45];
const TIMED: CronFreq[] = ["Every day", "Weekdays", "Every week", "Every month"];

export function defaultCronPreset(): CronPreset {
  return { freq: "Every day", n: 3, unit: "minutes", time: "9:00 AM", cron: "" };
}

export function cronFromPreset(input: CronPreset): string {
  if (input.freq === "Advanced") return input.cron.trim();
  if (input.freq === "Every hour") return "0 * * * *";
  if (input.freq === "Interval") {
    if (!Number.isInteger(input.n) || input.n < 1 || input.n > 365) throw new Error("間隔必須為 1–365 的整數");
    return `@every ${input.n}${({minutes:"m",hours:"h",days:"d"})[input.unit]}`;
  }
  const { hour, minute } = parseClock(input.time);
  if (input.freq === "Weekdays") return `${minute} ${hour} * * 1-5`;
  if (input.freq === "Every week") return `${minute} ${hour} * * 1`;
  if (input.freq === "Every month") return `${minute} ${hour} 1 * *`;
  return `${minute} ${hour} * * *`;
}

export function presetFromCron(cron: string): CronPreset {
  const base = defaultCronPreset();
  const interval = /^@every ([1-9]\d{0,2})([mhd])$/.exec(cron.trim());
  if (interval && Number(interval[1]) <= 365) return {...base, freq:"Interval", n:Number(interval[1]), unit:({m:"minutes",h:"hours",d:"days"} as const)[interval[2] as "m"|"h"|"d"]};
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return { ...base, freq: "Advanced", cron };
  const [minute, hour, day, month, dow] = parts;
  if (month !== "*") return { ...base, freq: "Advanced", cron };
  if (minute === "0" && hour === "*" && day === "*" && dow === "*") return { ...base, freq: "Every hour" };
  if (!/^\d+$/.test(minute) || !/^\d+$/.test(hour)) return { ...base, freq: "Advanced", cron };
  if (Number(hour) > 23 || Number(minute) > 59) return {...base, freq:"Advanced", cron};
  const time = formatClock(Number(hour), Number(minute));
  if (day === "*" && dow === "1-5") return { ...base, freq: "Weekdays", time };
  if (day === "*" && dow === "1") return { ...base, freq: "Every week", time };
  if (day === "1" && dow === "*") return { ...base, freq: "Every month", time };
  if (day === "*" && dow === "*") return { ...base, freq: "Every day", time };
  return { ...base, freq: "Advanced", cron };
}

function parseClock(time: string): { hour: number; minute: number } {
  const [rawH, rest] = time.split(":");
  const minute = Number((rest ?? "00").slice(0, 2));
  let hour = Number(rawH);
  if (/pm/i.test(time) && hour < 12) hour += 12;
  if (/am/i.test(time) && hour === 12) hour = 0;
  return { hour, minute };
}

function formatClock(hour: number, minute: number): string {
  const period = hour >= 12 ? "PM" : "AM";
  const h12 = hour % 12 === 0 ? 12 : hour % 12;
  return `${h12}:${String(minute).padStart(2, "0")} ${period}`;
}

function freqLabel(freq: CronFreq): string {
  return ({
    "Every hour": t("schedEveryHour"),
    "Every day": t("schedEveryDay"),
    "Weekdays": t("schedWeekdays"),
    "Every week": t("schedEveryWeek"),
    "Every month": t("schedEveryMonth"),
    "Interval": t("schedInterval"),
    "Advanced": t("schedAdvanced"),
  })[freq];
}

export function ScheduleList({
  items,
  onCreate,
  onOpen,
  onRun,
  runningId,
}: {
  items: ScheduleItem[];
  onCreate: () => void;
  onOpen: (item: ScheduleItem) => void;
  onRun: (item: ScheduleItem) => void;
  runningId?: string | null;
}) {
  return (
    <div className="sched-list">
      <div className="sched-list-head">
        <span>{t("schedules")}</span>
        <button type="button" className="icon-button" title={t("schedCreate")} onClick={onCreate}>+</button>
      </div>
      {items.length === 0 ? <p className="sched-empty">{t("schedEmpty")}</p> : items.map(item => (
        <div className="sched-row" key={item.id}>
          <button type="button" className="sched-row-main" onClick={() => onOpen(item)}>
            <i className={`sched-dot ${item.enabled ? "on" : "off"}`} />
            <span>
              <strong>{item.name}</strong>
              <small>{item.enabled ? item.human : t("schedPaused")}</small>
            </span>
          </button>
          <button type="button" className="outline" disabled={runningId === item.id} onClick={() => onRun(item)}>
            {runningId === item.id ? t("schedRunning") : t("schedRunNow")}
          </button>
        </div>
      ))}
    </div>
  );
}

export function ScheduleEditor({
  draft,
  timezone,
  saving,
  error,
  onChange,
  onBack,
  onSave,
  onDelete,
}: {
  draft: { name: string; instructions: string; enabled: boolean; preset: CronPreset };
  timezone: string;
  saving: boolean;
  error: string | null;
  onChange: (next: { name: string; instructions: string; enabled: boolean; preset: CronPreset }) => void;
  onBack: () => void;
  onSave: () => void;
  onDelete?: () => void;
}) {
  const preset = draft.preset;
  const times = TIMES.includes(preset.time) ? TIMES : [...TIMES, preset.time];
  const numbers = NUMBERS.includes(preset.n) ? NUMBERS : [...NUMBERS, preset.n].sort((a, b) => a - b);
  function patchPreset(partial: Partial<CronPreset>) {
    onChange({ ...draft, preset: { ...preset, ...partial } });
  }
  return (
    <div className="sched-editor">
      <div className="sched-editor-head">
        <button type="button" className="outline" onClick={onBack}>{t("back")}</button>
        <strong>{t("schedule")}</strong>
      </div>
      <label className="memory-toggle">
        <input type="checkbox" checked={draft.enabled} onChange={e => onChange({ ...draft, enabled: e.target.checked })} />
        {t("schedActive")}
      </label>
      <label>{t("name")}<input value={draft.name} onChange={e => onChange({ ...draft, name: e.target.value })} placeholder={t("schedNamePlaceholder")} /></label>
      <label>{t("schedInstruction")}<textarea rows={4} value={draft.instructions} onChange={e => onChange({ ...draft, instructions: e.target.value })} placeholder={t("schedInstructionPlaceholder")} /></label>
      <div className="sched-when">
        <span>{t("schedWhen")} <small>{timezone}</small></span>
        <div className="sched-card">
          <select value={preset.freq} onChange={e => patchPreset({ freq: e.target.value as CronFreq })} aria-label={t("schedWhen")}>
            {CRON_FREQS.map(freq => <option key={freq} value={freq}>{freqLabel(freq)}</option>)}
          </select>
          {preset.freq === "Interval" && <>
            <select value={String(preset.n)} onChange={e => patchPreset({ n: Number(e.target.value) })}>
              {numbers.map(n => <option key={n} value={n}>{n}</option>)}
            </select>
            <select value={preset.unit} onChange={e => patchPreset({ unit: e.target.value as CronUnit })}>
              <option value="minutes">{t("schedMinutes")}</option>
              <option value="hours">{t("schedHours")}</option>
              <option value="days">{t("schedDays")}</option>
            </select>
          </>}
          {TIMED.includes(preset.freq) && (
            <select value={preset.time} onChange={e => patchPreset({ time: e.target.value })}>
              {times.map(time => <option key={time} value={time}>{time}</option>)}
            </select>
          )}
          {preset.freq === "Advanced" && (
            <input className="sched-cron" value={preset.cron} placeholder="0 9 * * 1-5" onChange={e => patchPreset({ cron: e.target.value })} />
          )}
        </div>
      </div>
      {preset.freq === "Interval" && <small className="memory-help">固定經過時間；一天為 24 小時，不因月底或日光節約時間重置。</small>}
      {error && <div className="pane-error">{error}</div>}
      <div className="pane-actions">
        {onDelete && <button type="button" className="danger" disabled={saving} onClick={onDelete}>{t("delete")}</button>}
        <button type="button" className="primary" disabled={saving || !draft.name.trim() || !draft.instructions.trim() || (preset.freq === "Advanced" && !preset.cron.trim())} onClick={onSave}>{saving ? t("saving") : t("save")}</button>
      </div>
    </div>
  );
}
