// Shared by Chrome's isolated world and WebKit. State belongs to one Document;
// a navigation or detached node can never resolve to a replacement node.
((doc) => {
  let state = doc.__deviceBotRefs;
  if (!state) {
    state = { token: `${Date.now()}-${Math.random()}`, next: 0, ids: new WeakMap(), nodes: new Map() };
    Object.defineProperty(doc, '__deviceBotRefs', { value: state });
  }
  return {
    ref(node) {
      let id = state.ids.get(node);
      if (!id || !state.nodes.has(id)) { id = ++state.next; state.ids.set(node, id); state.nodes.set(id, new WeakRef(node)); }
      // Reap entries without retaining detached documents or DOM subtrees.
      for (const [key, value] of state.nodes) if (!value.deref()?.isConnected) state.nodes.delete(key);
      return `${state.token}:${id}`;
    },
    resolve(reference) {
      if (typeof reference !== 'string' || !reference.startsWith(`${state.token}:`)) throw new Error('Stale DOM document; inspect the element again');
      const node = state.nodes.get(Number(reference.slice(state.token.length + 1)))?.deref();
      if (!node?.isConnected || node.ownerDocument !== doc) throw new Error('Stale DOM element; inspect the element again');
      return node;
    }
  };
})
