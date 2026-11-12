(() => {
  const key = __KEY__;
  const state = { closed: false, error: null, answer: null, peer: null, media: null };
  const stop = () => {
    state.closed = true;
    state.media?.getTracks().forEach(track => track.stop());
    state.peer?.close();
    window.removeEventListener('resize', stop);
  };
  state.stop = stop;
  window[key] = state;
  (async () => {
    try {
      if (!navigator.mediaDevices?.getDisplayMedia || !window.CropTarget)
        throw new Error('Web Live requires HTTPS or localhost and Chrome tab capture support.');
      state.media = await navigator.mediaDevices.getDisplayMedia({audio:false,
        video:{displaySurface:'browser',frameRate:30}, preferCurrentTab:true,
        selfBrowserSurface:'include', surfaceSwitching:'exclude'});
      if (state.closed) {stop(); return;}
      const track = state.media.getVideoTracks()[0];
      if (!track?.cropTo) throw new Error('Choose This Tab for Web Live, then retry.');
      // Cropping is restricted to this document: selecting another tab cannot
      // silently display a different page while input controls this target.
      await track.cropTo(await CropTarget.fromElement(document.documentElement));
      if (state.closed) {stop(); return;}
      const peer = state.peer = new RTCPeerConnection({iceServers:[]});
      peer.onconnectionstatechange = () => {
        if (['failed','disconnected','closed'].includes(peer.connectionState)) stop();
      };
      track.onended = stop;
      peer.addTrack(track, state.media);
      await peer.setRemoteDescription({type:'offer',sdp:__OFFER__});
      await peer.setLocalDescription(await peer.createAnswer());
      await new Promise((resolve,reject) => {
        if (peer.iceGatheringState === 'complete') return resolve();
        const timer=setTimeout(() => reject(new Error('Web Live ICE gathering timed out')),10000);
        peer.onicegatheringstatechange=() => { if(peer.iceGatheringState==='complete'){clearTimeout(timer);resolve();} };
      });
      state.answer = {sdp:peer.localDescription.sdp,width:innerWidth,height:innerHeight};
      window.addEventListener('resize',stop,{once:true});
      setTimeout(() => {if(peer.connectionState!=='connected')stop();},20000);
    } catch (error) {state.error=String(error);stop();}
  })();
  return true;
})()
