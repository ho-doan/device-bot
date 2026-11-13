((doc, references, reference, action, text) => {
  const field = doc.activeElement;
  if (!field || field.disabled || field.readOnly || !(field.isContentEditable || /^(INPUT|TEXTAREA)$/.test(field.tagName))) throw new Error('Focus an editable field in this document first');
  if (reference !== null && references(doc).resolve(reference) !== field) throw new Error('The selected field no longer has focus');
  field.focus();
  if (action === 'clear') {
    if (field.value === '' || field.isContentEditable && field.textContent === '') return {ok:true};
    doc.execCommand('selectAll', false, null);
    if (!doc.execCommand('delete', false, null)) throw new Error('WebView clear command failed');
    if (field.isContentEditable ? field.textContent !== '' : field.value !== '') throw new Error('WebView clear did not empty the selected field');
  } else if (action === 'type') {
    if (!doc.execCommand('insertText', false, text)) throw new Error('WebView insert command failed');
  } else throw new Error('Unsupported DOM editing action');
  return {ok:true};
})
