"""Disposable loopback HTTP/TLS fixture; no external address, trust-store edit or device."""
import hashlib, http.server, json, pathlib, ssl, subprocess, sys, threading, time, urllib.parse
root = pathlib.Path(sys.argv[1])
(root / 'certificate.cnf').write_text('''[req]
distinguished_name=dn
x509_extensions=extensions
prompt=no
[dn]
CN=localhost
[extensions]
subjectAltName=DNS:localhost
basicConstraints=critical,CA:TRUE
keyUsage=critical,digitalSignature,keyEncipherment,keyCertSign
''')
subprocess.run(['/usr/bin/openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(root/'key.pem'), '-out', str(root/'cert.pem'), '-days', '1', '-config', str(root/'certificate.cnf')], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
servers = {}
class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def handle_one_request(self):
        try: super().handle_one_request()
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError): pass
    def do_GET(self):
        route = urllib.parse.urlsplit(self.path)
        path = route.path
        if path == '/slow-upload': time.sleep(3)
        if path == '/early-reject':
            self.answer(413, b'', {}); return
        data = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        record = dict(method=self.command, path=self.path, length=len(data), digest=hashlib.sha256(data).hexdigest(), contentType=self.headers.get('Content-Type'), cookie=self.headers.get('Cookie'), server=self.server.label)
        with lock:
            records.append(record)
            (root/'requests.json').write_text(json.dumps(records))
        delay = int(urllib.parse.parse_qs(route.query).get('delay', ['0'])[0])
        if delay: time.sleep(delay / 1000)
        status = 200; headers = {}
        body = json.dumps(record).encode()
        if path.startswith('/redirect/'):
            _, _, code, hops = path.split('/')
            status = int(code)
            headers['Location'] = (f'/redirect/{code}/{int(hops)-1}?delay={delay}' if int(hops) else '/final')
        elif path == '/relative':
            status = 302; headers['Location'] = 'final?relative=yes#ignored'
        elif path == '/absolute':
            status = 302; headers['Location'] = f'http://127.0.0.1:{servers["http"].server_port}/final'
        elif path in ['/tohttps', '/updown']:
            status = 302; headers['Location'] = f'https://localhost:{servers["tls"].server_port}/' + ('down' if path == '/updown' else 'final')
        elif path == '/down':
            status = 302; headers['Location'] = f'http://127.0.0.1:{servers["http"].server_port}/final'
        elif path == '/cookie/start':
            status = 302; headers['Location'] = '/final'; headers['Set-Cookie'] = 'ticket=redirect; Path=/'
        elif path == '/cookie/cross':
            status = 302; headers['Location'] = f'http://localhost:{servers["http"].server_port}/final'; headers['Set-Cookie'] = 'ticket=private; Path=/'
        elif path == '/api/system/info':
            body = json.dumps({'HostName': self.headers.get('Cookie', 'no-cookie')}).encode()
            headers['Set-Cookie'] = 'ticket=client; Path=/'
        elif path in ['/external', '/external-relative']:
            status = 302; headers['Location'] = ('http:' if path == '/external' else '') + '//192.0.2.1:9/never'
        elif path == '/file':
            status = 302; headers['Location'] = 'file:///private/tmp/not-an-fpp-response'
        elif path == '/missing': status = 302
        elif path == '/stall': time.sleep(3)
        elif path == '/large': body = b'x' * (16 * 1024 * 1024 + 1)
        elif path == '/large-header': headers['X-Large'] = 'x' * (64 * 1024)
        self.answer(status, body, headers)
    do_POST = do_GET
    def answer(self, status, body, headers):
        self.send_response(status)
        for name, value in headers.items(): self.send_header(name, value)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Connection', 'close'); self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args): pass
class Server(http.server.ThreadingHTTPServer):
    request_queue_size = 64  # Python's default of 5 makes macOS reset the tests' concurrent connections
records = []; lock = threading.Lock()
(root/'requests.json').write_text('[]')
for label in ['http', 'tls']:
    server = Server(('127.0.0.1', 0), Handler)
    server.label = label
    if label == 'tls':
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(str(root/'cert.pem'), str(root/'key.pem'))
        server.socket = context.wrap_socket(server.socket, server_side=True)
    servers[label] = server
    threading.Thread(target=server.serve_forever, daemon=True).start()
print(json.dumps({name: server.server_port for name, server in servers.items()}), flush=True)
sys.stdin.read()
for server in servers.values(): server.shutdown(); server.server_close()
