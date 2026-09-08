import http.server,ssl
PAGE=b'''<!doctype html><html><head><title>CUA saved-login fixture</title></head><body><h1>Saved login verification</h1><form><label>Email<input id="email" type="email" autocomplete="username"></label><label>Password<input id="password" type="password" autocomplete="current-password"></label><button type="submit">Sign in</button></form><p id="verification">Waiting for input</p><p id="result">Not submitted</p><script>document.querySelector('form').onsubmit=e=>{e.preventDefault();document.querySelector('#result').textContent='Submitted'};document.querySelectorAll('input').forEach(e=>e.oninput=()=>{document.querySelector('#verification').textContent=document.querySelector('#email').value==='cua@example.test'&&document.querySelector('#password').value==='fixture-only-123'?'Both fields verified':'CUA saved-login fixture'})</script></body></html>'''
class Handler(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  self.send_response(200);self.send_header('Content-Type','text/html');self.end_headers();self.wfile.write(PAGE)
server=http.server.HTTPServer(('127.0.0.1',8443),Handler)
ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);ctx.load_cert_chain('/tmp/login-cert.pem','/tmp/login-key.pem');server.socket=ctx.wrap_socket(server.socket,server_side=True);server.serve_forever()
