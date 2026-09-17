#!/usr/bin/env node

import { runLauncher } from '../lib/launcher.js';

await runLauncher('cadderd', import.meta.url);
