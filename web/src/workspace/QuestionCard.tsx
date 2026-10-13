import { useEffect, useState, type FormEvent } from "react";
import { api, type HumanQuestion } from "../api";
import { Hand, KeyRound, Monitor } from "../ui/icons";

/** Work the user did not start asks with a deadline (Grok Bot: about 10 minutes, then "Expired"). */
function Expiry({ at }: { at?: string }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => { if (!at) return; const timer = window.setInterval(() => setNow(Date.now()), 15000); return () => window.clearInterval(timer); }, [at]);
  if (!at) return null;
  const minutes = Math.max(0, Math.ceil((Date.parse(at) - now) / 60000));
  return <small className="question-expiry" title="這件工作不是你發起的，沒有回覆會自動過期">{minutes ? `${minutes} 分鐘內沒回覆會過期` : "即將過期"}</small>;
}

export function questionBlocksComposer(question?: HumanQuestion | null) {
  return !!question && (question.kind === "secret" || question.method === "approval");
}

function errorText(err: unknown) {
  return err instanceof Error ? err.message : String(err);
}

export function QuestionCard({ agentId, question, onDone, onOpenComputer }: { agentId: string; question: HumanQuestion; onDone: () => void; onOpenComputer: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [answers, setAnswers] = useState<Record<string, string | string[]>>({});
  const [custom, setCustom] = useState<Record<string, string>>({});
  const [values, setValues] = useState<Record<string, string>>({});
  const [text, setText] = useState("");
  const id = question.question_id || "";
  useEffect(() => { setAnswers({}); setCustom({}); setValues({}); setText(""); setError(""); setBusy(false); }, [id]);

  async function run(action: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true); setError("");
    try { await action(); onDone(); }
    catch (err) { setError(errorText(err)); setBusy(false); }
  }

  const prompt = (question.prompt || question.question || "").trim();
  const loginLike = /登入|密碼|帳號|驗證碼|login|password|sign in/i.test(prompt);

  if (question.method === "approval") {
    return (
      <section className="question-card approval" aria-label="需要你的允許">
        <header><Hand aria-hidden="true" /><strong>需要允許這項操作</strong><Expiry at={question.expires_at} /></header>
        {prompt ? <pre className="question-prompt">{prompt}</pre> : null}
        {error ? <p className="pane-error" role="alert">{error}</p> : null}
        <div className="question-actions">
          <button type="button" className="primary" disabled={busy} onClick={() => void run(() => api.answerQuestion(agentId, { question_id: id, choice: "once" }))}>允許這次</button>
          {question.options?.includes("一律允許") ? <button type="button" disabled={busy} title="以後同樣的操作不再問" onClick={() => void run(() => api.answerQuestion(agentId, { question_id: id, choice: "always" }))}>一律允許</button> : null}
          <button type="button" disabled={busy} onClick={() => void run(() => api.answerQuestion(agentId, { question_id: id, choice: "deny" }))}>拒絕</button>
        </div>
      </section>
    );
  }

  if (question.kind === "handoff" || question.kind === "box_help") {
    return (
      <section className="question-card handoff" aria-label="需要你協助操作電腦">
        <header><Hand aria-hidden="true" /><strong>需要你在電腦上協助</strong><Expiry at={question.expires_at} /></header>
        {prompt ? <p className="question-prompt">{prompt}</p> : null}
        {error ? <p className="pane-error" role="alert">{error}</p> : null}
        <div className="question-actions">
          <button type="button" disabled={busy} onClick={() => void run(() => api.handoverDone(agentId, question.task_id || "", id, "cancel"))}>停止這件工作</button>
          <button type="button" disabled={busy} onClick={onOpenComputer}><Monitor />打開電腦</button>
          <button type="button" className="primary" disabled={busy} onClick={() => void run(() => api.handoverDone(agentId, question.task_id || "", id))}>{busy ? "交還中…" : "完成，交還給 Bot"}</button>
        </div>
      </section>
    );
  }

  if (question.kind === "secret") {
    const fields = question.fields?.length ? question.fields : [{ name: "value", label: "秘密輸入", kind: "password" as const }];
    const submit = (event: FormEvent) => {
      event.preventDefault();
      void run(() => api.submitSecret(agentId, { question_id: id, value: values.value ?? values[fields[0].name], values }));
    };
    return (
      <form className="question-card secret" aria-label="需要秘密輸入" onSubmit={submit}>
        <header><KeyRound aria-hidden="true" /><strong>需要你輸入資料</strong><Expiry at={question.expires_at} /></header>
        {prompt ? <p className="question-prompt">{prompt}</p> : null}
        <div className="question-fields">
          {fields.map((field, index) => (
            <label key={field.name}>{field.label || field.name}
              <input autoFocus={index === 0} required type={field.kind === "text" ? "text" : "password"} autoComplete={field.kind === "text" ? "username" : field.kind === "otp" ? "one-time-code" : "current-password"}
                value={values[field.name] || ""} placeholder={field.placeholder} disabled={busy}
                onChange={event => setValues(current => ({ ...current, [field.name]: event.target.value }))} />
            </label>
          ))}
        </div>
        <small>直接交給 Bot 的電腦使用，不會寫進對話紀錄。也可以打開電腦自己輸入。</small>
        {error ? <p className="pane-error" role="alert">{error}</p> : null}
        <div className="question-actions">
          <button type="submit" className="primary" disabled={busy || fields.some(field => !values[field.name])}>{busy ? "傳送中…" : "送出"}</button>
          <button type="button" disabled={busy} onClick={onOpenComputer}><Monitor />打開電腦</button>
          <button type="button" disabled={busy} onClick={() => void run(() => api.submitSecret(agentId, { question_id: id, action: "cancel" }))}>不提供</button>
        </div>
      </form>
    );
  }

  const questions = question.hermes_questions || [];
  if (questions.length) {
    const ready = questions.every(q => custom[q.qid]?.trim() || (Array.isArray(answers[q.qid]) ? (answers[q.qid] as string[]).length : answers[q.qid]));
    const submit = (override?: Record<string, string | string[]>) => run(() => api.answerQuestion(agentId, {
      question_id: id,
      answers: Object.fromEntries(questions.map(q => [q.qid, override?.[q.qid] ?? (custom[q.qid]?.trim() || answers[q.qid] || null)])),
    }));
    const quick = questions.length === 1 && !questions[0].multi_select;
    return (
      <form className="question-card clarify" aria-label="Bot 的問題" onSubmit={event => { event.preventDefault(); void submit(); }}>
        <header><strong>{questions.length > 1 ? `${questions.length} 個問題需要你回答` : "Bot 想確認一件事"}</strong><Expiry at={question.expires_at} />{loginLike ? <button type="button" className="btn small ghost" onClick={onOpenComputer}><Monitor />打開電腦</button> : null}</header>
        {questions.map(q => (
          <fieldset key={q.qid} disabled={busy}>
            <legend>{q.question}</legend>
            {q.choices?.length ? (
              <div className="choice-list" role={q.multi_select ? "group" : "radiogroup"} aria-label={q.question}>
                {q.choices.map(choice => {
                  const picked = q.multi_select ? (Array.isArray(answers[q.qid]) && (answers[q.qid] as string[]).includes(choice)) : answers[q.qid] === choice;
                  return (
                    <button key={choice} type="button" className={`choice${picked ? " picked" : ""}`} role={q.multi_select ? "checkbox" : "radio"} aria-checked={picked}
                      onClick={() => {
                        if (quick) { setAnswers({ [q.qid]: choice }); void submit({ [q.qid]: choice }); return; }
                        setAnswers(old => {
                          const previous = Array.isArray(old[q.qid]) ? old[q.qid] as string[] : [];
                          return { ...old, [q.qid]: q.multi_select ? (previous.includes(choice) ? previous.filter(v => v !== choice) : [...previous, choice]) : choice };
                        });
                      }}>{choice}</button>
                  );
                })}
              </div>
            ) : null}
            <input aria-label={`${q.question}：自訂回答`} placeholder={q.choices?.length ? "或輸入其他回答" : "輸入你的回答"} value={custom[q.qid] || ""}
              onChange={event => setCustom(old => ({ ...old, [q.qid]: event.target.value }))} />
          </fieldset>
        ))}
        {error ? <p className="pane-error" role="alert">{error}</p> : null}
        <div className="question-actions">
          <button type="submit" className="primary" disabled={busy || !ready}>{busy ? "傳送中…" : "送出回答"}</button>
        </div>
      </form>
    );
  }

  return (
    <form className="question-card" aria-label="Bot 的問題" onSubmit={event => { event.preventDefault(); void run(() => api.answerQuestion(agentId, { question_id: id, text: text.trim() })); }}>
      <header><strong>Bot 需要你的回覆</strong><Expiry at={question.expires_at} /></header>
      {prompt ? <pre className="question-prompt">{prompt}</pre> : null}
      {question.options?.length ? (
        <div className="choice-list">
          {question.options.map(option => {
            const label = typeof option === "string" ? option : String((option as { label?: string })?.label || "");
            return label ? <button key={label} type="button" className="choice" disabled={busy} onClick={() => void run(() => api.answerQuestion(agentId, { question_id: id, text: label }))}>{label}</button> : null;
          })}
        </div>
      ) : null}
      <input aria-label="回覆內容" placeholder="輸入你的回覆" value={text} disabled={busy} onChange={event => setText(event.target.value)} />
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      <div className="question-actions"><button type="submit" className="primary" disabled={busy || !text.trim()}>送出</button></div>
    </form>
  );
}
