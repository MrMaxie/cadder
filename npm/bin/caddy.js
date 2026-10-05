#!/usr/bin/env node

import { runLauncher } from '../lib/launcher.js';

await runLauncher('caddy', import.meta.url);
