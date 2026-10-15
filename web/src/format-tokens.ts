/** Token counts the way people read big numbers here: 1.2 億, 3.4 萬. */
export const tokens = (n: number) => n >= 1e8 ? `${(n / 1e8).toFixed(1)} 億` : n >= 1e4 ? `${(n / 1e4).toFixed(1)} 萬` : n.toLocaleString();
