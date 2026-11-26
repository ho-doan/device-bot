((doc, references, reference, fractions) => {
  const node = references(doc).resolve(reference);
  if (node.disabled || node.getAttribute('aria-disabled') === 'true') throw new Error('DOM element is disabled');
  const rect = node.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) throw new Error('DOM element is hidden');
  return fractions.map(([fx, fy]) => {
    const x = rect.left + rect.width * fx;
    const y = rect.top + rect.height * fy;
    const hit = doc.elementFromPoint(x, y);
    if (!hit || !(node === hit || node.contains(hit))) throw new Error('DOM element is hidden or covered');
    return {x, y};
  });
})
