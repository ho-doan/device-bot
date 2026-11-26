((doc, references, reference) => {
  const node = references(doc).resolve(reference);
  if (node.disabled || node.getAttribute('aria-disabled') === 'true') throw new Error('DOM element is disabled');
  const rect = node.getBoundingClientRect();
  const x = (rect.left + rect.right) / 2;
  const y = (rect.top + rect.bottom) / 2;
  const hit = doc.elementFromPoint(x, y);
  if (rect.width <= 0 || rect.height <= 0 || !hit || !(node === hit || node.contains(hit))) {
    throw new Error('DOM element is hidden or covered');
  }
  return {x, y};
})
