// KP4 pre-FEC margin of the PAM-4 analyzer readout (catalog.js).
import test from 'node:test';
import assert from 'node:assert/strict';
import { fecMargin, KP4_BER_LIMIT } from '../src/lib/graph/catalog.js';

const pam4 = (o) => ({ level_mean: [0, 1, 2, 3], ser_method: 'semi_analytic', ...o });

test('KP4 limit is the IEEE 802.3 pre-FEC BER 2.4e-4', () => assert.equal(KP4_BER_LIMIT, 2.4e-4));

test('margin below and above the limit', () => {
  assert.deepEqual(fecMargin(pam4({ ber: 2.4e-7 })), { ok: true, text: 'OK ×1e3' });
  assert.deepEqual(fecMargin(pam4({ ber: 6e-5 })), { ok: true, text: 'OK ×4.0' });
  assert.deepEqual(fecMargin(pam4({ ber: 4.8e-3 })), { ok: false, text: 'FAIL ×20 over' });
});

test('SER of 0 uses the floor (shown as a lower bound); NRZ has no KP4 row', () => {
  assert.equal(fecMargin(pam4({ ber: 0, ser_floor: 2.3e-19 })).text, 'OK ×2e15+');
  assert.equal(fecMargin({ level_mean: [0, 1], ber: 1e-9 }), null);
});
