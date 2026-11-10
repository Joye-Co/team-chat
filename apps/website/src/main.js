// Scroll motion, the screenshot tours, the comparison tabs, the waitlist and the member builder. Plain DOM, no dependencies.
(function(){
  var root=document.documentElement;
  var reduce=window.matchMedia&&matchMedia("(prefers-reduced-motion: reduce)").matches;
  var io2="IntersectionObserver" in window;
  /* sliding pill behind the selected tab of a tab row */
  function slider(box){var ind=document.createElement("span");ind.className="ind still";box.insertBefore(ind,box.firstChild);box.classList.add("has-ind");
    function place(){var t=box.querySelector('[aria-selected="true"]');if(!t)return;ind.style.width=t.offsetWidth+"px";ind.style.height=t.offsetHeight+"px";ind.style.transform="translate("+t.offsetLeft+"px,"+t.offsetTop+"px)";if(box.scrollWidth>box.clientWidth+1)box.scrollTo({left:t.offsetLeft-(box.clientWidth-t.offsetWidth)/2,behavior:ind.classList.contains("still")?"auto":"smooth"});}
    place();requestAnimationFrame(function(){requestAnimationFrame(function(){ind.classList.remove("still");});});
    addEventListener("resize",place);addEventListener("load",place);if(document.fonts&&document.fonts.ready)document.fonts.ready.then(place);return place;}
  /* screenshot tours: each step names an image (data-img, without -light/-dark) and the part to ring (data-f: left, top, width, height in percent).
     The frame zooms toward that part; when the step's bar fills, the next step starts. Pointing at the tour or scrolling it out of view holds it. */
  [].forEach.call(document.querySelectorAll(".player"),function(el){
    var steps=[].slice.call(el.querySelectorAll(".ps")),pf=el.querySelector(".pf"),fit=el.classList.contains("fit"),layers={},order=[],cur=-1;
    pf.innerHTML="";
    steps.forEach(function(st){var k=st.dataset.img;if(layers[k])return;var d=document.createElement("div");d.className="pl-layer";
      d.innerHTML='<img class="l" alt="" src="'+k+'-light.webp"><img class="d" alt="" src="'+k+'-dark.webp">'+(fit?"":'<span class="pl-ring"></span>');pf.appendChild(d);layers[k]=d;order.push(k);});
    function geo(f){if(!f)return null;var p=f.split(",").map(function(v){return parseFloat(v)/100;});var phone=matchMedia("(max-width:760px)").matches;
      var s=Math.max(1,Math.min(phone?2.4:1.32,(phone?.94:.86)/Math.max(p[2],p[3])));
      var x=Math.min(0,Math.max(1-s,.5-s*(p[0]+p[2]/2))),y=Math.min(0,Math.max(1-s,.5-s*(p[1]+p[3]/2)));return {s:s,x:x*100,y:y*100,p:p};}
    function apply(L,g){var r=L.querySelector(".pl-ring");
      if(!g){L.style.transform="none";L.style.removeProperty("--s");if(r)r.style.opacity=0;return;}
      L.style.setProperty("--s",g.s);L.style.transform="translate("+g.x+"%,"+g.y+"%) scale("+g.s+")";
      if(r){var pad=.6,p=g.p;r.style.left=(p[0]*100-pad)+"%";r.style.top=(p[1]*100-pad)+"%";r.style.width=(p[2]*100+2*pad)+"%";r.style.height=(p[3]*100+2*pad)+"%";r.style.opacity=1;}}
    function go(i){var st=steps[i],L=layers[st.dataset.img],g=fit?null:geo(st.dataset.f);
      if(!L.classList.contains("on")){L.style.transition="none";var r=L.querySelector(".pl-ring");if(r)r.style.transition="none";apply(L,g);void L.offsetWidth;L.style.transition="";if(r)r.style.transition="";}else apply(L,g);
      order.forEach(function(k){layers[k].classList.toggle("on",layers[k]===L);});
      steps.forEach(function(x,j){x.classList.remove("on");x.querySelector("button").setAttribute("aria-current",j===i?"step":"false");});
      void st.offsetWidth;st.classList.add("on");cur=i;}
    steps.forEach(function(st,i){var b=st.querySelector("button");b.addEventListener("click",function(){go(i);});
      b.addEventListener("keydown",function(e){var n=e.key==="ArrowDown"||e.key==="ArrowRight"?1:e.key==="ArrowUp"||e.key==="ArrowLeft"?-1:0;if(n){e.preventDefault();var j=(i+n+steps.length)%steps.length;steps[j].querySelector("button").focus();go(j);}});
      st.querySelector(".bar").addEventListener("animationend",function(){if(cur===i)go((i+1)%steps.length);});});
    if(!reduce){el.classList.add("auto");
      el.addEventListener("mouseenter",function(){el.classList.add("held");});el.addEventListener("mouseleave",function(){el.classList.remove("held");});
      el.addEventListener("focusin",function(){el.classList.add("held");});el.addEventListener("focusout",function(){el.classList.remove("held");});
      if(io2){el.classList.add("away");new IntersectionObserver(function(es){es.forEach(function(e){el.classList.toggle("away",!e.isIntersecting);});},{threshold:.45}).observe(el);}}
    addEventListener("resize",function(){if(cur>=0&&!fit)apply(layers[steps[cur].dataset.img],geo(steps[cur].dataset.f));});
    go(0);
  });
  /* comparison: pick which alternative's panel shows */
  var cmp=document.querySelector(".cmp");
  if(cmp){cmp.classList.add("js");var ct=[].slice.call(cmp.querySelectorAll(".ctab")),vs=[].slice.call(cmp.querySelectorAll(".vs")),cPlace=slider(cmp.querySelector(".tabs"));
    var cpick=function(t){if(t.getAttribute("aria-selected")==="true")return;ct.forEach(function(x){x.setAttribute("aria-selected",x===t?"true":"false");});
      vs.forEach(function(v){v.classList.toggle("on",v.dataset.i===t.dataset.i);});cPlace();};
    ct.forEach(function(t,i){t.addEventListener("click",function(){cpick(t);});t.addEventListener("keydown",function(e){var n=e.key==="ArrowRight"?1:e.key==="ArrowLeft"?-1:0;if(n){var x=ct[(i+n+ct.length)%ct.length];x.focus();cpick(x);}});});}
  /* nav: mark the section in view */
  if(io2){var nl={};[].forEach.call(document.querySelectorAll(".links a"),function(a){nl[a.getAttribute("href").slice(1)]=a;});
    var spy=new IntersectionObserver(function(es){es.forEach(function(e){if(e.isIntersecting){for(var k in nl)nl[k].classList.toggle("on",k===e.target.id);}});},{rootMargin:"-40% 0px -55% 0px"});
    Object.keys(nl).forEach(function(k){var el=document.getElementById(k);if(el)spy.observe(el);});}
  if(reduce||!io2)return;
  root.classList.add("motion");
  /* scroll reveal */
  var groups=[[".center",0],[".chap-head>div",.12],[".trio>div",.1],[".player:not(.hero .player)",0],[".cmp .tabs",0],[".team",0],[".mem2 .txt>*",.06],[".mk",0],[".more3>div",.1],[".more4>div",.08],[".env",.08],[".split .shot",0],[".split .txt>*",.06],[".cta>*",.1]];
  var els=[];
  groups.forEach(function(g){var list=[].slice.call(document.querySelectorAll(g[0]));list.forEach(function(el,i){if(el.classList.contains("rv"))return;el.classList.add("rv");var sib=el.parentElement?[].indexOf.call(el.parentElement.children,el):i;el.style.setProperty("--dl",(Math.min(sib,6)*g[1])+"s");els.push(el);});});
  var io=new IntersectionObserver(function(es){es.forEach(function(e){if(e.isIntersecting){e.target.classList.add("in");io.unobserve(e.target);}});},{rootMargin:"0px 0px -12% 0px",threshold:.08});
  els.concat([].slice.call(document.querySelectorAll(".windows,.cmp"))).forEach(function(el){io.observe(el);});
  /* safety: never leave content hidden */
  setTimeout(function(){var h=innerHeight;els.forEach(function(el){if(el.getBoundingClientRect().top<h)el.classList.add("in");});},1200);
  var cm=document.querySelector(".cmp"),tick=false;
  function upd(){tick=false;if(cm&&!cm.classList.contains("in")&&cm.getBoundingClientRect().top<innerHeight*.8)cm.classList.add("in");}
  addEventListener("scroll",function(){if(!tick){tick=true;requestAnimationFrame(upd);}},{passive:true});upd();
})();
/* waitlist: GET /api/waitlist gives {count}; POST {email} books and gives the new {count}. The host serves that endpoint. */
(function(){
  var form=document.querySelector(".wl");if(!form)return;
  var KEY="waitlist",input=form.querySelector("input"),btn=form.querySelector("button"),msg=document.querySelector(".wl-msg"),counts=document.querySelectorAll(".wl-count");
  function count(n){if(typeof n!=="number"||n<10)return;[].forEach.call(counts,function(c){c.querySelector("b").textContent=n;c.hidden=false;});}
  function say(t,cls){msg.className="wl-msg"+(cls?" "+cls:"");msg.textContent=t;}
  function done(email){form.classList.add("done");say("已预约，上线时会发到 "+email,"ok");var b=document.createElement("button");b.type="button";b.textContent="换一个邮箱";
    b.addEventListener("click",function(){try{localStorage.removeItem(KEY);}catch(e){}form.classList.remove("done");say("");input.value="";input.focus();});msg.appendChild(b);}
  try{var saved=JSON.parse(localStorage.getItem(KEY)||"null");if(saved&&saved.email)done(saved.email);}catch(e){}
  fetch("/api/waitlist").then(function(r){return r.ok?r.json():null;}).then(function(d){if(d)count(d.count);}).catch(function(){});
  input.addEventListener("input",function(){input.removeAttribute("aria-invalid");if(msg.classList.contains("bad"))say("");});
  form.addEventListener("submit",function(e){e.preventDefault();var email=input.value.trim();
    if(!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)){input.setAttribute("aria-invalid","true");say("这个邮箱看起来不太对","bad");input.focus();return;}
    btn.disabled=true;say("");
    fetch("/api/waitlist",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({email:email})})
      .then(function(r){return r.json().catch(function(){return null;}).then(function(d){if(!r.ok)throw new Error("rejected");return d;});})
      .then(function(d){if(d)count(d.count);try{localStorage.setItem(KEY,JSON.stringify({email:email}));}catch(e){}done(email);})
      .catch(function(err){say(err&&err.message==="rejected"?"提交没成功，过一会儿再试一次":"网络不太顺，过一会儿再试一次","bad");})
      .then(function(){btn.disabled=false;});});
})();
/* build a member: the chips on the left drive the member card on the right */
(function(){
  var mk=document.querySelector(".mk");if(!mk)return;
  var P={forge:["Forge","开发","Claude","Claude Code",["GitHub","数据库"]],tata:["Tata","产品","GPT","Codex",["Notion","日历"]],nova:["Nova","增长","DeepSeek","你自己的",["Gmail","自定义 connector"]],watch:["Watch","运维","Gemini","Claude Code",["GitHub","数据库","日历"]]};
  var cur=P.forge,pres=[].slice.call(mk.querySelectorAll(".pre"));
  function $(id){return document.getElementById(id);}
  function opts(g){return [].slice.call(mk.querySelectorAll('[data-g="'+g+'"] .opt'));}
  function on(g){return opts(g).filter(function(b){return b.getAttribute("aria-pressed")==="true";}).map(function(b){return b.dataset.v;});}
  function put(id,t){var el=$(id);if(el.textContent===t)return;el.textContent=t;el.classList.remove("tick");void el.offsetWidth;el.classList.add("tick");}
  function render(){var m=on("m")[0],h=on("h")[0],t=on("t");put("mk-av",cur[0].charAt(0));put("mk-n",cur[0]);put("mk-role",cur[1]);put("mk-m",m);put("mk-h",h);put("mk-t",t.length?t.join("、"):"还没选");
    var text=cur[0]+" 用 "+m+" 跑在"+(h==="你自己的"?"你自己的 harness":h)+"上"+(t.length?"，能用"+(t.length>1?t.slice(0,-1).join("、")+"和"+t[t.length-1]:t[0]):"")+"。";
    /* a space between Chinese and Latin text, none before punctuation */
    put("mk-s",text.replace(/([\u4e00-\u9fff])([A-Za-z])/g,"$1 $2").replace(/([A-Za-z])([\u4e00-\u9fff])/g,"$1 $2"));}
  function set(g,vals){opts(g).forEach(function(b){b.setAttribute("aria-pressed",vals.indexOf(b.dataset.v)>=0?"true":"false");});}
  pres.forEach(function(b){b.addEventListener("click",function(){cur=P[b.dataset.p];pres.forEach(function(x){x.setAttribute("aria-selected",x===b?"true":"false");});set("m",[cur[2]]);set("h",[cur[3]]);set("t",cur[4]);render();});});
  ["m","h"].forEach(function(g){opts(g).forEach(function(b){b.addEventListener("click",function(){set(g,[b.dataset.v]);render();});});});
  opts("t").forEach(function(b){b.addEventListener("click",function(){b.setAttribute("aria-pressed",b.getAttribute("aria-pressed")==="true"?"false":"true");render();});});
})();
