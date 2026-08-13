import { moduleName } from './quickjs-spider-dep.js';

export default () => ({
  init(ext) {
    this.ext = ext;
    this.calls = 0;
  },

  async home(filter) {
    await Promise.resolve();
    this.calls += 1;
    return JSON.stringify({
      class: [{ type_id: 'fixture', type_name: 'QuickJS Fixture' }],
      meta: { calls: this.calls, ext: this.ext, filter, moduleName }
    });
  },

  sniffer() {
    return true;
  }
});
