import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { runInNewContext } from 'node:vm';
import { listScript, editScript, simulatorSocket, choosePage } from '../webview.mjs';

test('helper entrypoint also runs through a symlink',()=>{
  const dir=mkdtempSync(join(tmpdir(),'device-bot-helper-entry-'));
  try {
    const alias=join(dir,'helper.mjs');
    symlinkSync(fileURLToPath(new URL('../webview.mjs',import.meta.url)),alias);
    const result=spawnSync(process.execPath,[alias],{input:'{bad}\n',encoding:'utf8',timeout:5000});
    assert.equal(result.status,0,result.stderr);
    assert.equal(JSON.parse(result.stdout).code,'invalid');
  } finally {rmSync(dir,{recursive:true,force:true});}
});

function fixture() {
  const calls = [];
  const doc = { querySelectorAll: () => [], execCommand: (...args) => { calls.push(args); return true; } };
  const node = {
    ownerDocument: doc, isConnected: true, tagName:'INPUT', id:'field', value:'',
    getAttribute:()=>null, getBoundingClientRect:()=>({left:10,top:20,right:110,bottom:40,width:100,height:20}),
    focus(){doc.activeElement=this;}, scrollIntoView(){}, click(){calls.push('clicked');}, contains(other){return other===this;}
  };
  doc.body = {querySelectorAll:()=>[node]};
  doc.activeElement = node;
  doc.execCommand = (...args) => { calls.push(args); if (args[0] === 'insertText') node.value += args[2]; if (args[0] === 'delete') node.value = ''; return true; };
  doc.elementFromPoint = () => node;
  return {doc, node, calls};
}

test('WebView DOM query preserves app/page and survives scoped filtering', () => {
  const {doc} = fixture();
  const result = runInNewContext(listScript([], {appId:'com.example.fixture',page:{url:'https://fixture/',title:'Fixture'}}), {document:doc});
  assert.equal(result[0].webContext.appId, 'com.example.fixture');
  assert.equal(result[0].webContext.page.url, 'https://fixture/');
  assert.equal(typeof result[0].webContext.elementRef, 'string');
});

test('typing/clearing inside iframe executes against the child document', () => {
  const child = fixture();
  const root = fixture();
  root.doc.querySelectorAll = () => [{id:'checkout',getAttribute:()=>null,contentDocument:child.doc}];
  const env = {document:root.doc,CSS:{escape:x=>x}};
  runInNewContext(editScript('type','hello',[{css:'iframe#checkout'}]),env);
  runInNewContext(editScript('clear','',[{css:'iframe#checkout'}]),env);
  assert.equal(root.calls.length,0);
  assert.deepEqual(child.calls.map(call=>call[0]),['insertText','selectAll','delete']);
  child.doc.execCommand=()=>false;
  assert.throws(()=>runInNewContext(editScript('type','hello',[{css:'iframe#checkout'}]),env),/command failed/);
});

test('selected simulator UDID is passed to the resolver with no global fallback', async () => {
  const calls=[];
  assert.equal(await simulatorSocket('selected-udid',async udid=>{calls.push(udid);return null;}),null);
  assert.deepEqual(calls,['selected-udid']);
  assert.equal(await simulatorSocket('',()=>assert.fail('must not enumerate globally')),null);
});

test('activation rejects navigation, detached and replaced nodes without clicking a sibling', () => {
  const original=fixture();
  const result=runInNewContext(listScript([], {appId:'fixture'}),{document:original.doc});
  const ref=result[0].webContext.elementRef;
  const activate=doc=>runInNewContext(editScript('tap','',[],ref),{document:doc});
  activate(original.doc);
  assert.deepEqual(original.calls,['clicked']);
  original.node.isConnected=false;
  assert.throws(()=>activate(original.doc),/Stale DOM element/);
  const navigated=fixture();
  assert.throws(()=>activate(navigated.doc),/Stale DOM document/);
  assert.equal(navigated.calls.length,0);
});

test('covered nodes are not activated',()=>{
  const {doc,calls}=fixture();
  const rows=runInNewContext(listScript([],{appId:'fixture'}),{document:doc});
  doc.elementFromPoint=()=>({});
  assert.throws(()=>runInNewContext(editScript('tap','',[],rows[0].webContext.elementRef),{document:doc}),/covered/);
  assert.equal(calls.length,0);
});

test('DOM scrolling resolves the selected live node and rejects stale references',()=>{
  const {doc,node,calls}=fixture();
  node.scrollHeight=1000; node.clientHeight=100;
  node.scrollBy=options=>calls.push(options.top);
  const rows=runInNewContext(listScript([],{appId:'fixture'}),{document:doc});
  const script=editScript('scroll',JSON.stringify({dx:0,dy:50}),[],rows[0].webContext.elementRef);
  runInNewContext(script,{document:doc});
  assert.deepEqual(calls,[50]);
  node.isConnected=false;
  assert.throws(()=>runInNewContext(script,{document:doc}),/Stale DOM element/);
});

test('page choice is ambiguous until an explicit page selector resolves it',()=>{
  const pages=[{url:'one',title:'Page',id:'1.1'},{url:'two',title:'Page',id:'1.2'}];
  assert.equal(choosePage(pages,{}).code,'ambiguous');
  assert.equal(choosePage(pages,{pageUrl:'two'}).id,'1.2');
});

test('Chrome rejects scaled ancestors instead of returning unsafe offsets',()=>{
  const script=readFileSync(new URL('../../../robot-chrome/src/frame-owners.js',import.meta.url),'utf8');
  const parent={};
  const frame={id:'frame',src:'https://fixture/',parentElement:parent,getAttribute:()=>null,getBoundingClientRect:()=>({left:0,top:0,right:400,bottom:200})};
  const result=runInNewContext(script,{document:{querySelectorAll:()=>[frame]},getComputedStyle:node=>({transform:node===parent?'matrix(2, 0, 0, 2, 0, 0)':'none'})});
  assert.equal(result[0].unsupported,true);
});
