// A QR code encoder, ported from crates/openbmt-qr: byte mode only, error
// correction level M, the smallest version that fits and the mask with the
// lowest penalty (ties to the lower number), as the Rust encoder chooses
// them. It returns the module matrix; the page draws it. No dependencies.

// Error-correction codewords per block and number of blocks, level M,
// versions 1 to 40.
const ECC_PER_BLOCK = [
  10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28,
  28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
];
const BLOCKS = [
  1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25,
  26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49,
];

const side = (v) => v * 4 + 17;

function totalCodewords(v) {
  let n = (16 * v + 128) * v + 64;
  if (v >= 2) {
    const a = Math.floor(v / 7) + 2;
    n -= (25 * a - 10) * a - 55;
    if (v >= 7) n -= 36;
  }
  return Math.floor(n / 8);
}

const dataCodewords = (v) => totalCodewords(v) - ECC_PER_BLOCK[v - 1] * BLOCKS[v - 1];

function alignmentPositions(v) {
  if (v <= 1) return [];
  const n = Math.floor(v / 7) + 2;
  const step = v === 32 ? 26 : Math.floor((v * 4 + n * 2 + 1) / (n * 2 - 2)) * 2;
  const out = [6];
  let pos = side(v) - 7;
  for (let i = 1; i < n; i++) {
    out.splice(1, 0, pos);
    pos -= step;
  }
  return out;
}

// Reed-Solomon over GF(256) with the polynomial 0x11D.
function gfMul(x, y) {
  let z = 0;
  for (let i = 7; i >= 0; i--) {
    z = (z << 1) ^ ((z >> 7) * 0x11d);
    z ^= ((y >> i) & 1) * x;
  }
  return z & 0xff;
}

function rsDivisor(degree) {
  const result = new Array(degree).fill(0);
  result[degree - 1] = 1;
  let root = 1;
  for (let i = 0; i < degree; i++) {
    for (let j = 0; j < degree; j++) {
      const next = j + 1 < degree ? result[j + 1] : 0;
      result[j] = gfMul(result[j], root) ^ next;
    }
    root = gfMul(root, 2);
  }
  return result;
}

function rsRemainder(data, divisor) {
  const result = new Array(divisor.length).fill(0);
  for (const b of data) {
    const factor = b ^ result[0];
    result.shift();
    result.push(0);
    for (let i = 0; i < divisor.length; i++) result[i] ^= gfMul(divisor[i], factor);
  }
  return result;
}

function withErrorCorrection(data, v) {
  const nBlocks = BLOCKS[v - 1];
  const eccLen = ECC_PER_BLOCK[v - 1];
  const raw = totalCodewords(v);
  const nShort = nBlocks - (raw % nBlocks);
  const shortLen = Math.floor(raw / nBlocks);
  const div = rsDivisor(eccLen);
  const dataBlocks = [];
  const eccBlocks = [];
  let at = 0;
  for (let i = 0; i < nBlocks; i++) {
    const len = shortLen - eccLen + (i >= nShort ? 1 : 0);
    const block = data.slice(at, at + len);
    at += len;
    dataBlocks.push(block);
    eccBlocks.push(rsRemainder(block, div));
  }
  const out = [];
  const longest = Math.max(...dataBlocks.map((b) => b.length));
  for (let i = 0; i < longest; i++) {
    for (const b of dataBlocks) if (i < b.length) out.push(b[i]);
  }
  for (let i = 0; i < eccLen; i++) for (const b of eccBlocks) out.push(b[i]);
  return out;
}

// Format and version information (BCH codes). Level M is 0.
export function formatBits(mask) {
  const data = (0 << 3) | mask;
  let rem = data;
  for (let i = 0; i < 10; i++) rem = (rem << 1) ^ ((rem >> 9) * 0x537);
  return ((data << 10) | rem) ^ 0x5412;
}

export function versionBits(version) {
  let rem = version;
  for (let i = 0; i < 12; i++) rem = (rem << 1) ^ ((rem >> 11) * 0x1f25);
  return (version << 12) | rem;
}

class Grid {
  constructor(size) {
    this.size = size;
    this.dark = new Array(size * size).fill(false);
    this.func = new Array(size * size).fill(false);
  }

  setFunc(x, y, dark) {
    if (x < 0 || y < 0 || x >= this.size || y >= this.size) return;
    const i = y * this.size + x;
    this.dark[i] = dark;
    this.func[i] = true;
  }

  isFunc(x, y) {
    if (x < 0 || y < 0 || x >= this.size || y >= this.size) return true;
    return this.func[y * this.size + x];
  }

  finder(cx, cy) {
    for (let dy = -4; dy <= 4; dy++) {
      for (let dx = -4; dx <= 4; dx++) {
        const dist = Math.max(Math.abs(dx), Math.abs(dy));
        this.setFunc(cx + dx, cy + dy, dist !== 2 && dist !== 4);
      }
    }
  }

  alignment(cx, cy) {
    for (let dy = -2; dy <= 2; dy++) {
      for (let dx = -2; dx <= 2; dx++) {
        this.setFunc(cx + dx, cy + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
      }
    }
  }

  drawFunctionPatterns(version) {
    const size = this.size;
    for (let i = 0; i < size; i++) {
      this.setFunc(6, i, i % 2 === 0);
      this.setFunc(i, 6, i % 2 === 0);
    }
    this.finder(3, 3);
    this.finder(size - 4, 3);
    this.finder(3, size - 4);
    const pos = alignmentPositions(version);
    const last = pos.length - 1;
    pos.forEach((a, i) => {
      pos.forEach((b, j) => {
        const corner = (i === 0 && j === 0) || (i === 0 && j === last) || (i === last && j === 0);
        if (!corner) this.alignment(a, b);
      });
    });
    this.drawFormat(0);
    this.drawVersion(version);
  }

  drawFormat(mask) {
    const bits = formatBits(mask);
    const bit = (i) => ((bits >> i) & 1) !== 0;
    const size = this.size;
    for (let i = 0; i < 6; i++) this.setFunc(8, i, bit(i));
    this.setFunc(8, 7, bit(6));
    this.setFunc(8, 8, bit(7));
    this.setFunc(7, 8, bit(8));
    for (let i = 9; i < 15; i++) this.setFunc(14 - i, 8, bit(i));
    for (let i = 0; i < 8; i++) this.setFunc(size - 1 - i, 8, bit(i));
    for (let i = 8; i < 15; i++) this.setFunc(8, size - 15 + i, bit(i));
    this.setFunc(8, size - 8, true);
  }

  drawVersion(version) {
    if (version < 7) return;
    const bits = versionBits(version);
    const size = this.size;
    for (let i = 0; i < 18; i++) {
      const dark = ((bits >> i) & 1) !== 0;
      const a = size - 11 + (i % 3);
      const b = Math.floor(i / 3);
      this.setFunc(a, b, dark);
      this.setFunc(b, a, dark);
    }
  }

  place(codewords) {
    const size = this.size;
    const totalBits = codewords.length * 8;
    let i = 0;
    let right = size - 1;
    while (right >= 1) {
      if (right === 6) right = 5;
      for (let vert = 0; vert < size; vert++) {
        for (let j = 0; j < 2; j++) {
          const x = right - j;
          const upward = ((right + 1) & 2) === 0;
          const y = upward ? size - 1 - vert : vert;
          if (!this.isFunc(x, y) && i < totalBits) {
            this.dark[y * size + x] = ((codewords[i >> 3] >> (7 - (i & 7))) & 1) !== 0;
            i++;
          }
        }
      }
      right -= 2;
    }
  }

  applyMask(mask) {
    const size = this.size;
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const k = y * size + x;
        if (this.func[k]) continue;
        if (maskBit(mask, x, y)) this.dark[k] = !this.dark[k];
      }
    }
  }

  clone() {
    const g = new Grid(this.size);
    g.dark = this.dark.slice();
    g.func = this.func.slice();
    return g;
  }
}

function maskBit(mask, x, y) {
  switch (mask) {
    case 0:
      return (x + y) % 2 === 0;
    case 1:
      return y % 2 === 0;
    case 2:
      return x % 3 === 0;
    case 3:
      return (x + y) % 3 === 0;
    case 4:
      return (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0;
    case 5:
      return ((x * y) % 2) + ((x * y) % 3) === 0;
    case 6:
      return (((x * y) % 2) + ((x * y) % 3)) % 2 === 0;
    case 7:
      return (((x + y) % 2) + ((x * y) % 3)) % 2 === 0;
    default:
      return false;
  }
}

// The four penalty rules, as in the Rust encoder.
function runPenalty(line) {
  let total = 0;
  let run = 0;
  let prev = null;
  for (const m of line) {
    if (prev === m) {
      run++;
    } else {
      run = 1;
      prev = m;
    }
    if (run === 5) total += 3;
    else if (run > 5) total += 1;
  }
  return total;
}

const CORE = [true, false, true, true, true, false, true];

function finderLikePenalty(line) {
  let total = 0;
  for (let start = 0; start + 7 <= line.length; start++) {
    let match = true;
    for (let k = 0; k < 7; k++) {
      if (line[start + k] !== CORE[k]) {
        match = false;
        break;
      }
    }
    if (!match) continue;
    const lightBefore = start >= 4 && line.slice(start - 4, start).every((m) => !m);
    const after = line.slice(start + 7, start + 11);
    const lightAfter = after.length === 4 && after.every((m) => !m);
    if (lightBefore || lightAfter) total += 40;
  }
  return total;
}

function penalty(size, modules) {
  let total = 0;
  for (let y = 0; y < size; y++) {
    const row = modules.slice(y * size, (y + 1) * size);
    total += runPenalty(row) + finderLikePenalty(row);
  }
  for (let x = 0; x < size; x++) {
    const col = [];
    for (let y = 0; y < size; y++) col.push(modules[y * size + x]);
    total += runPenalty(col) + finderLikePenalty(col);
  }
  for (let y = 0; y + 1 < size; y++) {
    for (let x = 0; x + 1 < size; x++) {
      const p = modules[y * size + x];
      if (
        p === modules[y * size + x + 1] &&
        p === modules[(y + 1) * size + x] &&
        p === modules[(y + 1) * size + x + 1]
      ) {
        total += 3;
      }
    }
  }
  const dark = modules.filter(Boolean).length;
  const low = Math.floor((dark * 20) / modules.length);
  total += Math.min(Math.abs(low - 10), Math.abs(low + 1 - 10)) * 10;
  return total;
}

function countBits(version) {
  return version <= 9 ? 8 : 16;
}

/** The bytes as byte-mode segment bits, with terminator and byte padding. */
function segmentBits(data, version, capacity) {
  const out = [];
  const push = (value, n) => {
    for (let i = n - 1; i >= 0; i--) out.push(((value >> i) & 1) !== 0);
  };
  push(0b0100, 4);
  push(data.length, countBits(version));
  for (const b of data) push(b, 8);
  const terminator = Math.min(Math.max(capacity - out.length, 0), 4);
  for (let i = 0; i < terminator; i++) out.push(false);
  while (out.length % 8 !== 0) out.push(false);
  return out;
}

/**
 * Encodes bytes (byte mode, level M). Returns `{ version, mask, size,
 * modules }` where `modules` is an array of rows of booleans (true is dark),
 * without the quiet zone. Throws when the data does not fit version 40.
 */
export function encodeBytes(data) {
  if (!(data instanceof Uint8Array)) throw new Error('QR: bytes are needed');
  let version = 0;
  for (let v = 1; v <= 40; v++) {
    const capacity = dataCodewords(v) * 8;
    const fieldOk = data.length >> countBits(v) === 0;
    if (4 + countBits(v) + data.length * 8 <= capacity && fieldOk) {
      version = v;
      break;
    }
  }
  if (version === 0) throw new Error('QR: the data does not fit');
  const codewords = dataCodewords(version);
  const bits = segmentBits(data, version, codewords * 8);
  const dataCw = [];
  for (let i = 0; i < bits.length; i += 8) {
    let byte = 0;
    for (let k = 0; k < 8; k++) byte = (byte << 1) | (bits[i + k] ? 1 : 0);
    dataCw.push(byte);
  }
  const pad = [0xec, 0x11];
  for (let i = 0; dataCw.length < codewords; i++) dataCw.push(pad[i % 2]);
  const all = withErrorCorrection(dataCw, version);

  const base = new Grid(side(version));
  base.drawFunctionPatterns(version);
  base.place(all);
  let best = null;
  for (let m = 0; m < 8; m++) {
    const g = base.clone();
    g.applyMask(m);
    g.drawFormat(m);
    const p = penalty(g.size, g.dark);
    if (best === null || p < best.p) best = { p, m, g };
  }
  const size = side(version);
  const modules = [];
  for (let y = 0; y < size; y++) modules.push(best.g.dark.slice(y * size, (y + 1) * size));
  return { version, mask: best.m, size, modules };
}

/**
 * Grayscale pixels of a code with a quiet zone of 4 modules, black on
 * white, `scale` pixels a module. Returns `{ width, height, pixels }`.
 */
export function toLuma(code, scale) {
  const quiet = 4;
  const width = (code.size + quiet * 2) * scale;
  const pixels = new Uint8Array(width * width).fill(255);
  for (let y = 0; y < code.size; y++) {
    for (let x = 0; x < code.size; x++) {
      if (!code.modules[y][x]) continue;
      for (let dy = 0; dy < scale; dy++) {
        const row = ((y + quiet) * scale + dy) * width + (x + quiet) * scale;
        pixels.fill(0, row, row + scale);
      }
    }
  }
  return { width, height: width, pixels };
}

/** Runs of dark modules, as `[x, y, length]`, for drawing as rectangles. */
export function darkRuns(code) {
  const runs = [];
  for (let y = 0; y < code.size; y++) {
    let x = 0;
    while (x < code.size) {
      if (!code.modules[y][x]) {
        x++;
        continue;
      }
      let end = x;
      while (end < code.size && code.modules[y][end]) end++;
      runs.push([x, y, end - x]);
      x = end;
    }
  }
  return runs;
}
