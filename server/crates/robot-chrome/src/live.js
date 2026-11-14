(() => {
  const key = __KEY__;
  const state = { closed: false, error: null, errorName: null, phase: 'prepare', answer: null, peer: null, media: null };
  let iceTimer, connectionTimer, cancelIce;
  const stop = () => {
    state.closed = true;
    clearTimeout(iceTimer);
    clearTimeout(connectionTimer);
    cancelIce?.();
    cancelIce = null;
    // Take the resources before closing them: callbacks and repeated stop calls
    // must not release them twice. A late permission result can still be drained.
    const media = state.media;
    state.media = null;
    media?.getTracks().forEach(track => { track.onended = null; track.stop(); });
    if (state.peer) {
      state.peer.onconnectionstatechange = null;
      state.peer.onicegatheringstatechange = null;
      if (state.peer.connectionState !== 'closed') state.peer.close();
    }
    window.removeEventListener('resize', stop);
    if (!state.answer && !state.error) state.error = 'Web Live capture stopped before negotiation completed.';
  };
  state.stop = stop;
  window[key] = state;
  (async () => {
    try {
      if (!window.isSecureContext) {
        const page = location.href === 'about:blank' ? 'about:blank' : location.origin;
        throw new Error(`Open an HTTPS or localhost page in this Chrome case before starting Live (current page: ${page}). This is a page capture restriction, not a server build error.`);
      }
      if (typeof navigator.mediaDevices?.getDisplayMedia !== 'function')
        throw new Error('Screen sharing is unavailable in this Chrome page. Check browser screen-sharing support and permissions.');
      if (!window.CropTarget)
        throw new Error('This Chrome environment does not support Region Capture (CropTarget). Web Live needs a Chrome version with tab Region Capture support.');
      state.phase = 'capture';
      state.media = await navigator.mediaDevices.getDisplayMedia({audio:false,
        video:{displaySurface:'browser',frameRate:30}, preferCurrentTab:true,
        selfBrowserSurface:'include', surfaceSwitching:'exclude'});
      if (state.closed) {stop(); return;}
      state.phase = 'crop';
      const track = state.media.getVideoTracks()[0];
      if (track) track.onended = stop;
      if (!track?.cropTo) throw new Error('Choose This Tab for Web Live, then retry.');
      // Cropping is restricted to this document: selecting another tab cannot
      // silently display a different page while input controls this target.
      await track.cropTo(await CropTarget.fromElement(document.documentElement));
      if (state.closed) {stop(); return;}
      state.phase = 'negotiate';
      const peer = state.peer = new RTCPeerConnection({iceServers:[]});
      peer.onconnectionstatechange = () => {
        if (['failed','disconnected','closed'].includes(peer.connectionState)) stop();
      };
      peer.addTrack(track, state.media);
      await peer.setRemoteDescription({type:'offer',sdp:__OFFER__});
      if (state.closed) return;
      const answer = await peer.createAnswer();
      if (state.closed) return;
      await peer.setLocalDescription(answer);
      if (state.closed) return;
      await new Promise((resolve,reject) => {
        if (peer.iceGatheringState === 'complete') return resolve();
        cancelIce = () => reject(new Error('Web Live ICE gathering cancelled'));
        iceTimer=setTimeout(() => reject(new Error('Web Live ICE gathering timed out')),10000);
        peer.onicegatheringstatechange=() => { if(peer.iceGatheringState==='complete'){clearTimeout(iceTimer);cancelIce=null;peer.onicegatheringstatechange=null;resolve();} };
      });
      if (state.closed) return;
      state.answer = {sdp:peer.localDescription.sdp,width:innerWidth,height:innerHeight};
      window.addEventListener('resize',stop,{once:true});
      connectionTimer=setTimeout(() => {if(peer.connectionState!=='connected')stop();},20000);
    } catch (error) {state.error=String(error);state.errorName=error?.name ?? 'Error';stop();}
  })();
  return true;
})()
