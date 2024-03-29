// client-side js
(() => {

// lightweight .ready() replacement
function whenever(callback) {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', callback);
  } else {
    // yolo
    callback();
  }
}

// clipboard button handler
function clipboardHandler(button) {
  let target = document.getElementById( button.getAttribute('data-copy-target') );
  let copyButtons = document.getElementsByClassName('copy-button');
  if (navigator.clipboard) {
    navigator.clipboard.writeText(target.textContent).then(() => {
      resetButtonStatuses('success', button, copyButtons);
    }).catch(() => {
      resetButtonStatuses('fail', button, copyButtons);
    });
  } else {
    resetButtonStatuses('fail', button, copyButtons);
  }
}

// button status resetter, for sets of buttons that touch a global object like clipboard
function resetButtonStatuses(newStatus, activeButton, allButtons) {
  for (var i = 0; i < allButtons.length; i++) {
    let current = allButtons[i];
    let status = current.getElementsByClassName('status')[0];
    if (current === activeButton) {
      status.textContent = current.getAttribute(`data-status-${newStatus}`);
    } else {
      status.textContent = current.getAttribute('data-status-ready');
    }
  }
}

// Submit a dogear object to the endpoint of your choice. Returns a promise that resolves to a bool (success y/n).
function submitDogear(dest, dogObj, triggerElement) {
  return fetch(dest, {
    method: 'POST',
    credentials: 'include',
    headers: {'Content-Type': 'application/json', 'Accept': 'application/json'},
    body: JSON.stringify(dogObj)
  }).then(response => {
    if (response.ok) {
      replaceFragment('/fragments/dogears', '/', 'dogears-fragment', triggerElement);
      return true;
    } else {
      return false;
    }
  }).catch(_err => {
    return false;
  });
};

// u guessed it,
function deleteDogear(id, triggerElement) {
  triggerElement.classList.add('busy-fetching');
  fetch(`/api/v1/dogear/${id}`, {
    method: 'DELETE',
    credentials: 'include',
    headers: {'Content-Type': 'application/json', 'Accept': 'application/json'},
  }).then(() => {
    replaceFragment('/fragments/dogears', '/', 'dogears-fragment', triggerElement);
  });
}

function deleteToken(id, triggerElement) {
  triggerElement.classList.add('busy-fetching');
  fetch(`/tokens/${id}`, {
    method: 'DELETE',
    credentials: 'include',
  }).then(() => {
    replaceFragment('/fragments/tokens', '/account', 'tokens-fragment', triggerElement);
  })
}

let originalHistoryState = null;

// general-purpose way to update a fragment of a page
function replaceFragment(fragmentUrl, newPageUrl, fragmentElementId, triggerElement, method = 'GET') {
  let fragmentElement = document.getElementById(fragmentElementId);
  // Stash this _before_ revving up the spinner, so we don't get perma-spin on final back-nav.
  let previousText = fragmentElement.outerHTML;
  triggerElement.classList.add('busy-fetching');
  return fetch(fragmentUrl, {
    method,
    credentials: 'include',
  }).then(response => {
    response.text().then(text => {
      if (response.ok) {
        // Preserve original condition, if this is the first time we're making history:
        if (!originalHistoryState && !history.state) {
          originalHistoryState = {
            fragmentUrl: null,
            fragmentElementId,
            fragmentText: previousText,
          }
        }
        // Replace fragment, and update history state:
        fragmentElement.outerHTML = text;
        history.pushState({fragmentElementId, fragmentText: text}, '', newPageUrl);
      } else {
        fragmentElement.prepend(`Hmm, something went wrong: ${text}`);
      }
    });
  }).catch(err => {
    fragmentElement.prepend(`Hmm, something went wrong: ${err}`);
  }).finally(() => {
    triggerElement.classList.remove('busy-fetching');
  });
}

// history state: { fragmentUrl: string, fragmentElementId: string, fragmentText: string }

// Handle back/forward nav for partial page updates
window.addEventListener('popstate', function(e) {
  if (e.state && e.state.fragmentElementId && e.state.fragmentText) {
    let { fragmentElementId, fragmentText } = e.state;
    document.getElementById(fragmentElementId).outerHTML = fragmentText;
  } else if (!e.state && originalHistoryState) {
    let { fragmentElementId, fragmentText } = originalHistoryState;
    document.getElementById(fragmentElementId).outerHTML = fragmentText;
  }
});

// The big "clicking on buttons" listener
document.addEventListener('click', function(e){
  const that = e.target;
  if (that.matches('.help-reveal')) {
    // Help text toggle buttons:
    e.preventDefault();
    const helpTarget = document.getElementById( that.getAttribute('data-help-target') );
    helpTarget.classList.toggle('help-hidden');
    that.classList.toggle('help-reveal-active');
  } else if (that.matches('.pagination-link')) {
    // Don't mess with cmd/ctrl-click, only plain click!
    if (!e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      replaceFragment(
        that.getAttribute('data-fragment-url'),
        that.getAttribute('href'),
        that.getAttribute('data-fragment-element-id'),
        that
      ).catch(() => {
        document.location.href = that.getAttribute('href');
      });
    }
  } else if (that.matches('#generate-personal-bookmarklet')) {
    // This one's a one-off, so just hardcode everything.
    replaceFragment(
      '/fragments/personalmark?csrf_token=' + encodeURIComponent(that.getAttribute('data-csrf-token')),
      '/install',
      'generate-personal-bookmarklet-fragment',
      that,
      'POST'
    );
  } else if (that.matches('.tabs .tab')) {
    e.preventDefault();
    document.getElementById(that.getAttribute('data-target'))
      .setAttribute('data-show', that.getAttribute('data-show'));
    let tabs = document.querySelectorAll('.tabs .tab');
    for (let i = 0; i < tabs.length; i++) {
      tabs[i].classList.remove('active');
    }
    that.classList.add('active');
  } else if (that.matches('.copy-button')) {
    // Clipboard copy buttons:
    e.preventDefault();
    clipboardHandler(that);
  } else if (that.matches('.really-delete.delete-dogear')) {
    // Armed delete buttons (order matters, must check this before the "really" one):
    e.preventDefault();
    deleteDogear(that.getAttribute('data-dogear-id'), that);
  } else if (that.matches('.really-delete.token-delete')) {
    e.preventDefault();
    deleteToken(that.getAttribute('data-token-id'), that);
  } else if (that.matches('.delete-button')) {
    // Unarmed delete buttons:
    e.preventDefault();
    that.classList.add('really-delete');
    that.innerText = 'REALLY delete';
  } else {
    // Disarm delete buttons when clicking elsewhere:
    const reallies = this.getElementsByClassName('really-delete');
    // it's a live HTMLCollection, so we have to run the loop backwards. Comedy.
    for (var i = reallies.length - 1; i >= 0; i--) {
      reallies[i].innerText = 'Delete';
      reallies[i].classList.remove('really-delete');
    }
  }
});

// Manual "dogear a URL" form on homepage
document.addEventListener('submit', function(e){
  const that = e.target;
  if (that.matches('#update-dogear')) {
    e.preventDefault();
    submitDogear(
      '/api/v1/update',
      {current: that.elements['current'].value},
      that
    ).then(success => {
      if (success) {
        that.elements['current'].value = '';
      } else {
        document.location.href = '/mark/' + encodeURIComponent(that.elements['current'].value);
      }
    });
  }
});

// OK, here's all the stuff where I need to know the page state before doing something:
whenever(() => {
  // Reveal copy buttons if they're functional
  if (navigator.clipboard) {
    document.body.classList.add('copy-buttons-enabled');
  }

  // "Returning to site in..." countdown timer after dogearing something
  const countdownIndicator = document.getElementById('countdown');
  if (countdownIndicator) {
    // we're redirecting soon.
    var count = 3;
    function tick() {
      if (count > 0) {
        countdownIndicator.innerText = count.toString();
        setTimeout(tick, count * 300);
      } else {
        document.location.href = countdownIndicator.getAttribute('data-returnto');
      }
      count--;
    }
    tick();
  }

  // Creating new dogear: Suggest the domain name as the default prefix, but let them customize it
  // if the same domain hosts several sites.
  // TODO: push some of this logic back into the server side.
  const createForm = document.getElementById('create-dogear');
  if (createForm && createForm.elements['prefix'] && createForm.elements['prefix'].value) {
    const prefix = createForm.elements['prefix'];
    const changePrefix = document.getElementById('change-prefix');

    const prefixHost = (new URL(prefix.defaultValue)).host + '/';

    prefix.value = prefixHost;
    prefix.readOnly = true;
    prefix.classList.add('read-only');
    changePrefix.style.display = 'inline-block'; // 'cause it's hidden by default.

    changePrefix.addEventListener('click', function(_e){
      this.style.display = 'none';
      prefix.readOnly = false;
      prefix.classList.remove('read-only');
      prefix.value = prefix.defaultValue.replace(/^https?:\/\//, '');
      prefix.focus();
    });
  }

}); // end whenever()
})(); // that's a wrap
