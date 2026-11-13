// 이 컴퓨터에 설치된 uv 를 앱에 넣을 위치로 복사한다 (Tauri externalBin 규칙: 이름-<타깃>).
// 앱은 실행 파일 옆의 uv 로 LiteLLM 실행 환경을 설치하므로, 사용자는 uv 를 따로 설치할 필요가 없다.
import { execFileSync } from "node:child_process";
import { copyFileSync, chmodSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const win = process.platform === "win32";
const host = /host: (\S+)/.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))[1];
const found = execFileSync(win ? "where" : "which", ["uv"], { encoding: "utf8" }).split(/\r?\n/)[0].trim();
if (!found || !existsSync(found)) {
  console.error("uv 를 찾을 수 없습니다. https://docs.astral.sh/uv/ 에서 설치하세요.");
  process.exit(1);
}
const dir = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "binaries");
mkdirSync(dir, { recursive: true });
const dest = join(dir, `uv-${host}${win ? ".exe" : ""}`);
copyFileSync(found, dest);
if (!win) chmodSync(dest, 0o755);
console.log(`uv → ${dest}`);
