(() => Array.from(document.querySelectorAll('iframe,frame')).map((element) => {
    const bounds = element.getBoundingClientRect();
    let unsupported = false;
    const style = getComputedStyle(element);
    for (let current = element; current; current = current.parentElement) {
      const currentStyle = getComputedStyle(current);
      const transform = currentStyle.transform || 'none';
      if (currentStyle.zoom && currentStyle.zoom !== 'normal' && Number(currentStyle.zoom) !== 1) unsupported = true;
      if (transform !== 'none') {
        const match = transform.match(/^matrix\(([^)]+)\)$/);
        if (!match) unsupported = true;
        else {
            const parts = match[1].split(',').map((part) => Number(part.trim()));
            unsupported ||= parts.length !== 6 || Math.abs(parts[0] - 1) > 0.0001 || Math.abs(parts[3] - 1) > 0.0001 || Math.abs(parts[1]) > 0.0001 || Math.abs(parts[2]) > 0.0001;
        }
      }
    }
    const border = (side) => Number.parseFloat(style[side]) || 0;
    const id = element.id || '';
    const name = element.getAttribute('name') || '';
    const css = id
        ? `iframe#${id.replace(/"/g, '\\"')}`
        : (name ? `iframe[name="${name.replace(/"/g, '\\"')}"]` : '');
    return {
        css, name, url: element.src || '',
        left: bounds.left + border('borderLeftWidth'),
        top: bounds.top + border('borderTopWidth'),
        right: bounds.right - border('borderRightWidth'),
        bottom: bounds.bottom - border('borderBottomWidth'),
        unsupported
    };
}))()
