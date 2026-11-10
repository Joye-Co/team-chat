// Scroll motion, the card gallery and the clickable app demos. Plain DOM, no dependencies.
(function(){
  var root=document.documentElement;
  var reduce=window.matchMedia&&matchMedia("(prefers-reduced-motion: reduce)").matches;
  /* card gallery */
  var tabs=[].slice.call(document.querySelectorAll(".tab")),l=document.getElementById("v-l"),d=document.getElementById("v-d"),c=document.getElementById("v-c"),card=document.querySelector(".view .card");
  function show(t){var k=t.dataset.k;[l,d].forEach(function(im,i){im.width=+t.dataset.w;im.height=+t.dataset.h;im.alt=t.textContent+"卡片";im.src="img/card-"+k+(i?"-dark":"-light")+".webp";});
    c.textContent="";var b=document.createElement("b");b.textContent=t.textContent;c.appendChild(b);c.appendChild(document.createTextNode("："+t.dataset.d));}
  function pick(t){if(t.getAttribute("aria-selected")==="true")return;tabs.forEach(function(x){x.setAttribute("aria-selected",x===t?"true":"false");});
    if(reduce){show(t);return;}card.classList.add("swap");setTimeout(function(){show(t);requestAnimationFrame(function(){card.classList.remove("swap");});},220);}
  tabs.forEach(function(t,i){t.addEventListener("click",function(){pick(t);});t.addEventListener("keydown",function(e){var n=e.key==="ArrowRight"?1:e.key==="ArrowLeft"?-1:0;if(n){var x=tabs[(i+n+tabs.length)%tabs.length];x.focus();pick(x);}});});
  window.addEventListener("load",function(){tabs.forEach(function(t){["light","dark"].forEach(function(m){var im=new Image();im.src="img/card-"+t.dataset.k+"-"+m+".webp";});});});
  if(reduce||!("IntersectionObserver" in window))return;
  root.classList.add("motion");
  /* scroll reveal */
  var groups=[[".center",0],[".chap-head>div",.12],[".trio>div",.1],[".app",.05],[".one",0],[".shot:not(.hero .shot)",0],[".gal .tabs",0],[".view",.1],[".mem",0],[".env",.08],[".tile",.1],[".blk",.12],[".conn",.12],[".split .txt>*",.08],[".cta>*",.1]];
  var els=[];
  groups.forEach(function(g){var list=[].slice.call(document.querySelectorAll(g[0]));list.forEach(function(el,i){if(el.classList.contains("rv"))return;el.classList.add("rv");var sib=el.parentElement?[].indexOf.call(el.parentElement.children,el):i;el.style.setProperty("--dl",(Math.min(sib,6)*g[1])+"s");els.push(el);});});
  document.querySelectorAll(".cycle .st .n").forEach(function(n,i){n.style.setProperty("--dl",(.25+i*.28)+"s");});
  var io=new IntersectionObserver(function(es){es.forEach(function(e){if(e.isIntersecting){e.target.classList.add("in");io.unobserve(e.target);}});},{rootMargin:"0px 0px -12% 0px",threshold:.08});
  els.concat([].slice.call(document.querySelectorAll(".cycle,.windows"))).forEach(function(el){io.observe(el);});
  /* safety: never leave content hidden */
  setTimeout(function(){var h=innerHeight;els.forEach(function(el){if(el.getBoundingClientRect().top<h)el.classList.add("in");});},1200);
  /* hero screenshot: tilted at first, flattens as you scroll */
  var hs=document.querySelector(".hero .shot"),tick=false;
  function upd(){tick=false;var r=hs.getBoundingClientRect(),p=Math.min(Math.max((innerHeight-r.top)/(innerHeight*.9),0),1);hs.style.setProperty("--tilt",(14*(1-p)).toFixed(2)+"deg");hs.style.setProperty("--hs",(.94+.06*p).toFixed(3));}
  addEventListener("scroll",function(){if(!tick){tick=true;requestAnimationFrame(upd);}},{passive:true});upd();
})();
(function(){
  var SB=[[1.2,6.3,15.4,3.5,"ask","交代或搜索 ⌘K"],[1.2,11,16,3.4,"desk","首页"],[1.2,14.6,16,3.4,"artifacts-v2","全部产出"],[1.2,18.1,16,3.4,"members-v2","成员"],[1.2,22.4,16,2.6,"group-v2","群「开发」"],[1.2,25.2,16,3.4,"live","Forge 正在做的话题"],[1.2,28.8,16,3.4,"topic-v2","打开这个话题"]];
  var G={
    "desk":{sb:1,h:[[69.7,49.6,5.5,3.1,"desk-answered","批准合并"],[65.6,49.6,3.5,3.1,"topic-v2","打开这个话题"],[20,78.8,55.6,5.2,"live","看 Forge 在做什么"],[78.8,46.3,20.4,3.3,"topic-artifact-pane","打开重连测试报告"],[78.8,49.9,20.4,3.3,"artifact-v2","打开 10 月数据月报"]]},
    "desk-answered":{sb:1,h:[[78.8,46.3,20.4,3.3,"topic-artifact-pane","打开重连测试报告"],[78.8,49.9,20.4,3.3,"artifact-v2","打开 10 月数据月报"]]},
    "topic-v2":{sb:1,h:[[61.2,1.9,3.7,3.4,"topic-artifact-pane","产出",1],[66.3,1.9,4.6,3.4,"side-chat","单独问 Jojo",1],[69.4,81.7,5.5,3.2,"topic-merged","批准合并"],[24.2,65.8,42.2,3.3,"topic-artifact-pane","打开重连测试报告"]]},
    "side-chat":{sb:1,h:[[96.5,1.6,2.5,4,"topic-v2","关闭",1],[61,2,3.8,3.2,"topic-artifact-pane","产出",1],[69.3,81.5,5.6,3.3,"topic-merged","批准合并"]]},
    "topic-artifact-pane":{sb:1,h:[[96.5,1.6,2.5,4,"topic-v2","关闭",1],[52.6,2,4.2,3.2,"side-chat","单独问 Jojo",1],[55.4,80.6,5.6,3.2,"topic-merged","批准合并"]]},
    "topic-merged":{sb:1,h:[[71.5,82,3.4,3.2,"topic-v2","撤回"]]},
    "artifacts-v2":{sb:1,h:[[20,26,78.5,3.2,"artifact-v2","打开 10 月数据月报"],[20,20.5,78.5,3.2,"topic-artifact-pane","打开重连测试报告"],[75,2,2.5,3.2,"artifacts-grid","图标视图",1]]},
    "artifacts-grid":{sb:1,h:[[78,2,2.6,3.2,"artifacts-v2","列表视图",1]]},
    "artifact-v2":{sb:1,h:[[20.5,1.8,2.2,3.5,"artifacts-v2","返回产出列表",1],[78.6,34.7,20,3.2,"artifact-v2-old","看第 2 版"]]},
    "artifact-v2-old":{sb:1,h:[[20.5,1.8,2.2,3.5,"artifacts-v2","返回产出列表",1],[78.6,31.1,20,3.2,"artifact-v2","回到第 3 版"]]},
    "members-v2":{sb:1,h:[[19.6,10.9,19.6,5.7,"members-memory","看 Forge 记得的事"],[43.3,31.3,25,4,"live","看 Forge 在做什么"],[44,68,52.5,3.4,"topic-v2","打开 PR #61 的话题"],[44,73.4,52.5,3.4,"topic-artifact-pane","打开重连测试报告"]]},
    "members-memory":{sb:1,h:[]},
    "live":{sb:1,h:[[86.4,94.9,12.5,3.2,"live-takeover","接管"]]},
    "live-takeover":{sb:1,h:[[86.4,94.9,12.5,3.2,"live","交还"]]},
    "group-v2":{sb:1,h:[[20.8,14.9,78.4,4.4,"topic-v2","打开这个话题"],[20.8,38.9,78.4,5,"live","打开这个话题"]]},
    "ask":{sb:0,h:[[0,0,100,100,"@back","",0,1]]}
  };
  var cache={};
  function pre(k){["light","dark"].forEach(function(m){var u="img/ui/"+k+"-"+m+".webp";if(!cache[u]){var im=new Image();im.src=u;cache[u]=im;}});}
  function layer(k){var d=document.createElement("div");d.className="layer";d.innerHTML='<img class="l" alt="" src="img/ui/'+k+'-light.webp"><img class="d" alt="" src="img/ui/'+k+'-dark.webp">';return d;}
  document.querySelectorAll(".demo").forEach(function(el){
    var start=el.dataset.start,cur=start,prev=start,stack=el.querySelector(".stack"),bar=el.nextElementSibling,reset=bar&&bar.querySelector(".reset"),base=stack.innerHTML;
    stack.innerHTML="";var first=document.createElement("div");first.innerHTML=base;first.style.position="relative";stack.appendChild(first);
    var hsWrap=document.createElement("div");hsWrap.style.cssText="position:absolute;inset:0";el.appendChild(hsWrap);
    function draw(){hsWrap.innerHTML="";var st=G[cur]||{h:[]};var list=(st.sb?SB:[]).concat(st.h);
      list.forEach(function(h){var b=document.createElement("button");b.type="button";b.className="hs"+(h[7]?" full":"")+(h[1]<8?" below":"");
        b.style.cssText="left:"+h[0]+"%;top:"+h[1]+"%;width:"+h[2]+"%;height:"+h[3]+"%";
        if(h[5]){b.setAttribute("aria-label",h[5]);var t=document.createElement("span");t.className="tip";t.textContent=h[5];b.appendChild(t);}else b.setAttribute("aria-label","关闭");
        b.addEventListener("click",function(e){e.stopPropagation();go(h[4]==="@back"?prev:h[4]);});hsWrap.appendChild(b);pre(h[4]);});
      if(reset)reset.hidden=(cur===start);}
    function go(k){if(!G[k]||k===cur)return;if(k!=="ask")prev=cur;else prev=cur;var L=layer(k);L.classList.add("enter");stack.appendChild(L);
      var im=L.querySelector(getComputedStyle(L.querySelector(".l")).display==="none"?".d":".l");
      function show(){setTimeout(function(){L.classList.remove("enter");setTimeout(function(){L.style.transition="none";while(stack.children.length>2)stack.removeChild(stack.children[1]);},420);},30);}
      if(im.complete)show();else{im.addEventListener("load",show);im.addEventListener("error",show);}
      cur=k;draw();}
    if(reset)reset.addEventListener("click",function(){go(start);});
    draw();
    var io=new IntersectionObserver(function(es){es.forEach(function(e){if(e.isIntersecting){el.classList.add("hint");Object.keys(G).forEach(pre);io.disconnect();}});},{threshold:.3});
    io.observe(el);
  });
})();
