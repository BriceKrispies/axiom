/**
 * Bakery worker entry.
 *
 * Deliberately two lines: the plumbing is in src/core/bakeryhost.js and the
 * recipes are in src/bakers.js, so this file exists only to be the module URL
 * that `new Worker(...)` can point at. Vite needs a real entry module for that;
 * it cannot instantiate a worker from a registry object.
 */

import { serveBakes } from './core/bakeryhost.js';
import { BAKERS } from './bakers.js';

serveBakes(BAKERS);
