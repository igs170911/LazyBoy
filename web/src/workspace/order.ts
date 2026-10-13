/**
 * A sidebar section dragged onto another: dropped on one further down it goes after it, further up before it, so
 * one step down and the last place are both reachable.
 */
export function moveGroup(order: string[], dragged: string, target: string): string[] {
  const from = order.indexOf(dragged), to = order.indexOf(target);
  if (from < 0 || to < 0 || from === to) return order;
  const ids = order.filter(id => id !== dragged);
  ids.splice(ids.indexOf(target) + (from < to ? 1 : 0), 0, dragged);
  return ids;
}
