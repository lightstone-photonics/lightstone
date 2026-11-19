// Small path helpers for nested parameter objects ("vcsel.wavelength").

export const get = (obj, path) => path.split('.').reduce((o, k) => (o == null ? undefined : o[k]), obj);
export function set(obj, path, value) {
  const ks = path.split('.');
  const last = ks.pop();
  const t = ks.reduce((o, k) => (o[k] ??= {}), obj);
  t[last] = value;
}
