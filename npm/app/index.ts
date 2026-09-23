#!/usr/bin/env node

import { spawnSync } from "child_process";

function getExePath() {
  const arch = process.arch;
  let os = process.platform as string;
  let extension = "";
  
  if (["win32", "cygwin"].includes(process.platform)) {
    os = "windows";
    extension = ".exe";
  }

  try {
    return require.resolve(`@julienguillot/gradual-${os}-${arch}/bin/app${extension}`);
  } catch (e) {
    throw new Error(
      `Couldn't find application binary inside node_modules for ${os}-${arch}. Ensure optional dependencies are installed.`
    );
  }
}

function run() {
  const args = process.argv.slice(2);
  const processResult = spawnSync(getExePath(), args, { stdio: "inherit" });
  
  if (processResult.error) {
    console.error(processResult.error.message);
    process.exit(1);
  }
  
  process.exit(processResult.status ?? 1);
}

run();
