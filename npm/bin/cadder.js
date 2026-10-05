#!/usr/bin/env node

import { runLauncher } from '../lib/launcher.js';

await runLauncher('cadder', import.meta.url);
