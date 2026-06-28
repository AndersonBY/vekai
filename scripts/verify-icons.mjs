import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const tauriConfigPath = path.join(root, "src-tauri", "tauri.conf.json");
const tauriConfig = JSON.parse(fs.readFileSync(tauriConfigPath, "utf8"));
const icons = tauriConfig.bundle?.icon ?? [];

let failed = false;
for (const icon of icons) {
  if (!icon.toLowerCase().endsWith(".png")) {
    continue;
  }

  const iconPath = path.join(root, "src-tauri", icon);
  const data = fs.readFileSync(iconPath);
  const signature = data.subarray(0, 8).toString("hex");
  const chunkType = data.subarray(12, 16).toString("ascii");
  if (signature !== "89504e470d0a1a0a" || chunkType !== "IHDR") {
    console.error(`${icon}: expected a PNG file with an IHDR chunk`);
    failed = true;
    continue;
  }

  const bitDepth = data[24];
  const colorType = data[25];
  if (bitDepth !== 8) {
    console.error(`${icon}: expected 8-bit PNG, got bitDepth=${bitDepth} colorType=${colorType}`);
    failed = true;
  }
}

if (failed) {
  process.exit(1);
}

console.log(`Verified ${icons.filter((icon) => icon.toLowerCase().endsWith(".png")).length} PNG icons.`);
