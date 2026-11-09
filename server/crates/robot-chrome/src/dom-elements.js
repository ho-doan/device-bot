Array.from(document.querySelectorAll('body *')).slice(0, 500).map((element) => {
            const bounds = element.getBoundingClientRect();
            const tag = element.tagName.toLowerCase();
            const type = (element.getAttribute('type') || '').toLowerCase();
            const role = (element.getAttribute('role') || '').toLowerCase();
            let elementType = '';
            if (role === 'checkbox' || type === 'checkbox') elementType = 'checkBox';
            else if (role === 'switch') elementType = 'switch';
            else if (role === 'radio' || type === 'radio') elementType = 'radio';
            else if (role === 'slider' || type === 'range') elementType = 'slider';
            else if (tag === 'textarea' || tag === 'input' || role === 'textbox' || role === 'searchbox') elementType = 'textField';
            else if (tag === 'button' || role === 'button') elementType = 'button';
            else if (tag === 'a' || role === 'link') elementType = 'link';
            else if (tag === 'img' || role === 'img') elementType = 'image';
            const selectable = type === 'checkbox' || type === 'radio' || role === 'checkbox' || role === 'radio' || role === 'switch';
            return {
              className: tag, resourceId: element.id || '',
              text: (element.innerText || element.textContent || '').trim(),
              contentDescription: element.getAttribute('aria-label') || element.getAttribute('title') || '',
              value: typeof element.value === 'string' ? element.value : null,
              elementType,
              left: bounds.left, top: bounds.top, right: bounds.right, bottom: bounds.bottom,
              clickable: typeof element.onclick === 'function' || role === 'button' || ['a', 'button', 'input', 'select', 'textarea'].includes(tag),
              enabled: !element.disabled,
              focused: document.activeElement === element,
              checked: selectable ? !!element.checked : null,
              selected: element.getAttribute('aria-selected') === 'true' ? true : (element.getAttribute('aria-selected') === 'false' ? false : null)
            };
        })
