#!/usr/bin/env node
import { main } from './index.js';

const exitCode = await main();
process.exitCode = exitCode;
