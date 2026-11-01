/**
 * Essential runtime polyfills for Chrome 81+ compatibility.
 * Chrome 81 was released in April 2020 (V8 8.1).
 * Features polyfilled here:
 * - String.prototype.replaceAll (Chrome 85)
 * - Object.hasOwn (Chrome 93)
 * - Array.prototype.at (Chrome 92)
 * - crypto.randomUUID (Chrome 92)
 * - AbortSignal.timeout (Chrome 103)
 * - Promise.any (Chrome 85)
 * - structuredClone (Chrome 98)
 */

// 1. String.prototype.replaceAll
if (!String.prototype.replaceAll) {
  String.prototype.replaceAll = function (searchValue: any, replaceValue: any): string {
    if (searchValue instanceof RegExp) {
      if (!searchValue.global) {
        throw new TypeError('String.prototype.replaceAll called with a non-global RegExp');
      }
      return this.replace(searchValue, replaceValue);
    }
    const str = String(this);
    const search = String(searchValue);
    if (typeof replaceValue === 'function') {
      return str.split(search).reduce((acc, part, i) => {
        if (i === 0) return part;
        return acc + replaceValue(search, acc.length, str) + part;
      }, '');
    }
    return str.split(search).join(String(replaceValue));
  };
}

// 2. Object.hasOwn
if (!Object.hasOwn) {
  Object.hasOwn = function (object: any, property: PropertyKey): boolean {
    if (object == null) {
      throw new TypeError('Cannot convert undefined or null to object');
    }
    return Object.prototype.hasOwnProperty.call(Object(object), property);
  };
}

// 3. Array.prototype.at
if (!Array.prototype.at) {
  Array.prototype.at = function (index: number) {
    let k = Math.trunc(index) || 0;
    if (k < 0) k += this.length;
    if (k < 0 || k >= this.length) return undefined;
    return this[k];
  };
}

// 4. crypto.randomUUID
if (typeof crypto !== 'undefined' && !crypto.randomUUID) {
  crypto.randomUUID = function (): `${string}-${string}-${string}-${string}-${string}` {
    const bytes = new Uint8Array(16);
    if (crypto.getRandomValues) {
      crypto.getRandomValues(bytes);
    } else {
      for (let i = 0; i < 16; i++) bytes[i] = Math.floor(Math.random() * 256);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}` as any;
  };
}

// 5. AbortSignal.timeout
if (typeof AbortSignal !== 'undefined' && !(AbortSignal as any).timeout) {
  (AbortSignal as any).timeout = function (ms: number) {
    const controller = new AbortController();
    setTimeout(() => {
      try {
        controller.abort(new DOMException('The operation timed out.', 'TimeoutError'));
      } catch {
        controller.abort();
      }
    }, ms);
    return controller.signal;
  };
}

// 6. Promise.any
if (!Promise.any) {
  Promise.any = function <T>(promises: Iterable<T | PromiseLike<T>>): Promise<T> {
    return new Promise((resolve, reject) => {
      const arr = Array.from(promises);
      if (arr.length === 0) {
        const error = typeof (window as any).AggregateError === 'function'
          ? new (window as any).AggregateError([], 'All promises were rejected')
          : new Error('All promises were rejected');
        return reject(error);
      }
      let rejectedCount = 0;
      const errors = new Array(arr.length);
      arr.forEach((promise, i) => {
        Promise.resolve(promise).then(
          (val) => resolve(val),
          (err) => {
            errors[i] = err;
            rejectedCount++;
            if (rejectedCount === arr.length) {
              const aggError = typeof (window as any).AggregateError === 'function'
                ? new (window as any).AggregateError(errors, 'All promises were rejected')
                : new Error('All promises were rejected');
              (aggError as any).errors = errors;
              reject(aggError);
            }
          }
        );
      });
    });
  };
}

// 7. structuredClone
if (typeof window !== 'undefined' && !(window as any).structuredClone) {
  (window as any).structuredClone = function <T>(obj: T): T {
    if (obj === undefined) return undefined as any;
    return JSON.parse(JSON.stringify(obj));
  };
}

export {};
