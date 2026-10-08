import { appendFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { basename } from 'node:path';
import Module from 'node:module';

// The async owned fixture must not let Node execute the Caddy command as a script.
Module.runMain = () => {};

const behavior = JSON.parse(readFileSync(`${process.execPath}.json`, 'utf8')) as {
  version?: string;
  modules: string[];
  fail?: string;
  output?: string;
  marker?: string;
  releaseVersion?: string;
};
const args = process.argv.slice(1);
// Node resolves argv[1] as a script path before preloads; the disposable native fake intercepts it.
args[0] = basename(args[0]!);
if (behavior.marker) appendFileSync(behavior.marker, JSON.stringify(args) + '\n');
if (behavior.fail === args[0]) process.exit(7);
function finish(): void {
  if (args[0] === 'version') process.stdout.write(behavior.version ?? 'v2.11.4 h1:fixture\n');
  else if (args[0] === 'list-modules')
    process.stdout.write(
      JSON.stringify(
        behavior.modules.map((module_name) => ({ module_name, module_type: 'standard' })),
      ),
    );
  else if (args[0] === 'marker') writeFileSync(args[1]!, 'executed');
  else process.stdout.write(behavior.output ?? '{}');
  process.exit(0);
}
if (args[0] === 'version' && behavior.releaseVersion) {
  const timer = setInterval(() => {
    if (existsSync(behavior.releaseVersion!)) {
      clearInterval(timer);
      finish();
    }
  }, 20);
} else finish();
