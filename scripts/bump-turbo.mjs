import { join } from "node:path";
import cp from "node:child_process";
import { promisify } from "node:util";
import { readdir, stat } from "node:fs/promises";
import { fileURLToPath } from "url";

const currentModuleURL = import.meta.url;
const currentModulePath = fileURLToPath(currentModuleURL);

const currentDirectory = new URL(".", "file://" + currentModulePath).pathname;

async function main() {
  const examplesDir = join(currentDirectory, "..", "examples");
  const examples = await readdir(examplesDir);

  // We don't try to use Promise.all here because @turbo/codemod does not like dirty
  // git state, so we have to run them one at a time.
  for (const example of examples) {
    const dir = join(examplesDir, example);
    if ((await stat(dir)).isDirectory()) {
      const cmd1 = `npx @turbo/codemod update ./examples/${example}`;
      console.log(`Running ${cmd1}`);
      const { stderr, stdout } = await promisify(cp.exec)(cmd1, {
        stderr: "inherit",
        stdout: "inherit",
        stdin: "inherit",
      });
      console.log(stdout);
      console.log(stderr);

      const cmd2 = `git commit -am "chore(examples): bump turbo in examples/${example}"`;
      console.log(`Running ${cmd2}`);
      await promisify(cp.exec)(cmd2, { stdio: "inherit" });
    }
  }
}

main();
