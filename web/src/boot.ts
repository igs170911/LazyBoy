/** Fades out the opening screen in index.html once the app has something to show (signed in, or the login form). */
export function dismissBoot() {
  const boot = document.getElementById("boot");
  if (!boot || boot.classList.contains("gone")) return;
  boot.classList.add("gone");
  window.setTimeout(() => boot.remove(), 320);
}
