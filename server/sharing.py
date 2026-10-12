"""Portable connector structure and known-secret removal, matching LazyBoy2."""
import copy
import re
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

REQUIRED="<REQUIRED>"
FLAG=re.compile(r"^--?[a-zA-Z][a-zA-Z-]*$")
PACKAGE=re.compile(r"^(?:@[a-z0-9_.-]+/)?[a-z0-9_.-]+(?:@[a-zA-Z0-9_.^~*-]+)?$")


def portable_connectors(entries):
    clean=copy.deepcopy(entries)
    secrets=[]
    for cfg in clean.values():
        for field in ("headers","env"):
            values=cfg.get(field,{})
            for value in values.values():
                if value:
                    secrets.append(value)
                    if value.startswith("Bearer "): secrets.append(value[7:])
            cfg[field]={k:"" for k in values}
        if cfg.get("url"):
            before=cfg["url"]
            try:
                url=urlsplit(before)
                secrets.extend(v for v in (url.username,url.password) if v)
                query=parse_qsl(url.query,keep_blank_values=True)
                secrets.extend(v for _,v in query if v)
                path=url.path
                if path not in {"","/","/mcp","/mcp/","/sse","/sse/"}:
                    secrets.append(path.lstrip("/"));path="/"+REQUIRED
                cfg["url"]=urlunsplit((url.scheme,url.netloc.rsplit("@",1)[-1],path,urlencode([(k,REQUIRED) for k,_ in query]),""))
            except ValueError: cfg["url"]=REQUIRED
        if cfg.get("command"):
            launcher=cfg["command"]
            if launcher not in {"npx","uvx","node","python","python3","uv","docker"}:
                secrets.append(launcher);cfg["command"]=REQUIRED
            package_used=False
            args=[]
            for arg in cfg.get("args",[]):
                if FLAG.fullmatch(arg): args.append(arg);continue
                if launcher in {"npx","uvx"} and not package_used and PACKAGE.fullmatch(arg):
                    package_used=True;args.append(arg);continue
                key,sep,value=arg.partition("=")
                secrets.append(value if sep and value else arg)
                args.append(key+"="+REQUIRED if sep and FLAG.fullmatch(key) else REQUIRED)
                package_used=True
            cfg["args"]=args
        cfg["enabled"]=False
        # Keep only public configuration fields. Runtime auth/trust state is private.
        for field in set(cfg)-{"url","command","args","headers","env","enabled"}: cfg.pop(field)
    return clean,secrets


def scrub_known(value, secrets):
    if isinstance(value,str):
        for secret in sorted(set(s for s in secrets if isinstance(s,str) and s),key=len,reverse=True): value=value.replace(secret,REQUIRED)
        return value
    if isinstance(value,list): return [scrub_known(v,secrets) for v in value]
    if isinstance(value,dict): return {k:scrub_known(v,secrets) for k,v in value.items()}
    return value
