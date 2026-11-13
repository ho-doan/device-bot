((doc, references, reference, dx, dy) => {
  let node = references(doc).resolve(reference);
  if (!Number.isFinite(dx) || !Number.isFinite(dy)) throw new Error('Invalid DOM scroll delta');
  while (node && !(node.scrollHeight > node.clientHeight || node.scrollWidth > node.clientWidth)) node = node.parentElement;
  node ||= doc.scrollingElement;
  if (!node) throw new Error('No scrollable DOM element is available');
  node.scrollBy({left:dx, top:dy, behavior:'instant'});
  return {ok:true};
})
