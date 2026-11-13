((doc, references, reference) => {
  const node = references(doc).resolve(reference);
  if (node.disabled || node.getAttribute('aria-disabled') === 'true') throw new Error('DOM element is disabled');
  node.scrollIntoView({block:'center', inline:'center'});
  const rect = node.getBoundingClientRect();
  const hit = doc.elementFromPoint((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2);
  if (rect.width <= 0 || rect.height <= 0 || !hit || !(node === hit || node.contains(hit))) throw new Error('DOM element is hidden or covered');
  node.focus({preventScroll:true});
  node.click();
  return {ok:true};
})
