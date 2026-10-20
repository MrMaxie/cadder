#!/usr/bin/env node
import { runCli } from '../cli/run.ts';
process.exitCode = await runCli(process.argv.slice(2));
