import { useState } from "react";
import { api, type AgentGroup, type AgentRow } from "../api";
import { Avatar } from "../avatar";
import { Check } from "../ui/icons";
import { Modal, ModalHeader } from "../ui/overlay";
import { avatarProps } from "./Sidebar";

type TeamResult = Awaited<ReturnType<typeof api.manageTeam>>;

export function GroupEditor({ group, agents, onClose, onSaved }: {
  group?: AgentGroup;
  agents: AgentRow[];
  onClose: () => void;
  onSaved: (data: TeamResult) => void;
}) {
  const [name, setName] = useState(group?.name || "");
  const [members, setMembers] = useState<string[]>(() => group ? agents.filter(agent => agent.group_id === group.id).map(agent => agent.id) : []);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const toggle = (id: string) => setMembers(current => current.includes(id) ? current.filter(item => item !== id) : [...current, id]);

  async function save() {
    if (!name.trim() || busy) return;
    setBusy(true); setError("");
    try {
      onSaved(await api.manageTeam({ action: group ? "update_group" : "create_group", group_id: group?.id, name: name.trim(), member_ids: members }));
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }

  return (
    <Modal label={group ? "編輯分區" : "新增分區"} onClose={() => { if (!busy) onClose(); }} className="group-dialog">
      <form onSubmit={event => { event.preventDefault(); void save(); }}>
        <ModalHeader title={group ? "編輯分區" : "新增分區"} onClose={onClose} disabled={busy} />
        <label className="field">
          <span>分區名稱</span>
          <input autoFocus value={name} maxLength={40} placeholder="例如：研究、行銷、日常" onChange={event => setName(event.target.value)} />
        </label>
        <fieldset className="member-picker">
          <legend>成員 <small>{members.length} / {agents.length}</small></legend>
          <ul>
            {agents.map(agent => {
              const picked = members.includes(agent.id);
              return (
                <li key={agent.id}>
                  <button type="button" className={picked ? "picked" : ""} aria-pressed={picked} onClick={() => toggle(agent.id)}>
                    <Avatar {...avatarProps(agent)} size={28} gaze={false} />
                    <span>{agent.name}</span>
                    {picked ? <Check aria-hidden="true" /> : null}
                  </button>
                </li>
              );
            })}
          </ul>
        </fieldset>
        {error ? <p className="pane-error" role="alert">{error}</p> : null}
        <div className="dialog-actions">
          <button type="button" disabled={busy} onClick={onClose}>取消</button>
          <button type="submit" className="primary" disabled={busy || !name.trim()}>{busy ? "儲存中…" : group ? "儲存" : "建立分區"}</button>
        </div>
      </form>
    </Modal>
  );
}

export function MoveDialog({ agent, groups, onClose, onSaved }: {
  agent: AgentRow;
  groups: AgentGroup[];
  onClose: () => void;
  onSaved: (data: TeamResult) => void;
}) {
  const [target, setTarget] = useState(agent.group_id || "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function save() {
    setBusy(true); setError("");
    try {
      onSaved(await api.manageTeam({ action: "move_agent", agent_id: agent.id, group_id: target }));
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }
  return (
    <Modal label={`移動 ${agent.name}`} onClose={() => { if (!busy) onClose(); }} className="group-dialog">
      <ModalHeader title={`把「${agent.name}」移到…`} onClose={onClose} disabled={busy} />
      <div className="choice-stack" role="radiogroup" aria-label="分區">
        {[{ id: "", name: "未分區" }, ...groups].map(group => (
          <button type="button" key={group.id || "none"} role="radio" aria-checked={target === group.id} className={target === group.id ? "picked" : ""} onClick={() => setTarget(group.id)}>
            <span>{group.name}</span>{target === group.id ? <Check aria-hidden="true" /> : null}
          </button>
        ))}
      </div>
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      <div className="dialog-actions">
        <button type="button" disabled={busy} onClick={onClose}>取消</button>
        <button type="button" className="primary" disabled={busy || target === (agent.group_id || "")} onClick={() => void save()}>{busy ? "移動中…" : "移動"}</button>
      </div>
    </Modal>
  );
}
