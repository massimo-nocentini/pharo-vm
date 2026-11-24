// node -r bh-capture.cjs: dumps the branch counters of the instrumented module at exit into $BH_OUT
const fs=require('fs'); let ex=null;
function grab(i){ try { if (i.exports.__bh0) ex=i.exports; } catch(e){} return i; }
const I=WebAssembly.Instance; WebAssembly.Instance=function(m,imp){ return grab(new I(m,imp)); }; WebAssembly.Instance.prototype=I.prototype;
const inst=WebAssembly.instantiate; WebAssembly.instantiate=async function(m,imp){ const r=await inst(m,imp); grab(r.instance||r); return r; };
process.on('exit',()=>{ if(!ex||!process.env.BH_OUT) return; const a=[]; for(let i=0;;i++){ const g=ex['__bh'+i]; if(!g) break; a.push(Number(g.value)); } fs.writeFileSync(process.env.BH_OUT,JSON.stringify(a)); });
