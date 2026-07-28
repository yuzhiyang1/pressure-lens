import { readFile, writeFile } from "node:fs/promises";

const publicKey = process.env.PRESSURE_LENS_UPDATER_PUBKEY?.trim();
const certificateThumbprint = process.env.WINDOWS_CERTIFICATE_THUMBPRINT?.trim();
if (!publicKey) {
  throw new Error("缺少 PRESSURE_LENS_UPDATER_PUBKEY，不能生成可验证的更新包");
}
if (!certificateThumbprint) {
  throw new Error("缺少 WINDOWS_CERTIFICATE_THUMBPRINT，拒绝生成未签名安装包");
}

const basePath = new URL("../src-tauri/tauri.conf.json", import.meta.url);
const outputPath = new URL("../src-tauri/tauri.generated.conf.json", import.meta.url);
const config = JSON.parse(await readFile(basePath, "utf8"));

// 生成文件不会提交；公开公钥被同时嵌入 Rust 和 Tauri 的更新产物配置。
config.bundle.createUpdaterArtifacts = true;
config.bundle.windows.certificateThumbprint = certificateThumbprint;
config.bundle.windows.digestAlgorithm = "sha256";
config.bundle.windows.timestampUrl = "http://timestamp.digicert.com";
config.plugins = {
  ...(config.plugins ?? {}),
  updater: {
    pubkey: publicKey,
    endpoints: [
      "https://github.com/yuzhiyang1/pressure-lens/releases/latest/download/latest.json",
    ],
    windows: {
      installMode: "passive",
    },
  },
};

await writeFile(outputPath, `${JSON.stringify(config, null, 2)}\n`, "utf8");
console.log(outputPath.pathname);
