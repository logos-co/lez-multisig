#!/usr/bin/env python3
"""Proxy: rewrites /api/codex/v1/* → /api/storage/v1/* on real Logos Storage.
Also unwraps multipart/form-data uploads to raw octet-stream (real Codex expects raw bytes).
"""
from http.server import HTTPServer, BaseHTTPRequestHandler
import urllib.request, urllib.error, re, io

UPSTREAM = "http://127.0.0.1:8888"

def extract_multipart(content_type, body):
    m = re.search(r'boundary=([^\s;]+)', content_type)
    if not m:
        return body, 'application/octet-stream'
    boundary = ('--' + m.group(1)).encode()
    parts = body.split(boundary)
    for part in parts[1:]:
        if part in (b'--', b'--\r\n', b'\r\n--'):
            continue
        if b'\r\n\r\n' in part:
            _, content = part.split(b'\r\n\r\n', 1)
            content = content.rstrip(b'\r\n').rstrip(b'--')
            if content:
                return content, 'application/octet-stream'
    return body, 'application/octet-stream'

class Proxy(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args): pass

    def _rewrite(self, path):
        return path.replace('/api/codex/v1', '/api/storage/v1', 1)

    def do_POST(self):
        target = UPSTREAM + self._rewrite(self.path)
        length = int(self.headers.get('Content-Length', 0))
        body = self.rfile.read(length)
        content_type = self.headers.get('Content-Type', '')

        # Unwrap multipart → raw bytes for real Codex
        if 'multipart/form-data' in content_type:
            body, content_type = extract_multipart(content_type, body)

        req = urllib.request.Request(target, data=body, method='POST')
        req.add_header('Content-Type', content_type)
        req.add_header('Content-Length', str(len(body)))
        try:
            with urllib.request.urlopen(req) as resp:
                data = resp.read()
                self.send_response(resp.status)
                self.send_header('Content-Type', resp.headers.get('Content-Type', 'text/plain'))
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)
        except urllib.error.HTTPError as e:
            body = e.read()
            self.send_response(e.code)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    def do_GET(self):
        target = UPSTREAM + self._rewrite(self.path)
        try:
            with urllib.request.urlopen(target) as resp:
                data = resp.read()
                self.send_response(resp.status)
                self.send_header('Content-Type', resp.headers.get('Content-Type', 'application/octet-stream'))
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)
        except urllib.error.HTTPError as e:
            self.send_response(e.code); self.end_headers()

if __name__ == '__main__':
    s = HTTPServer(('127.0.0.1', 8080), Proxy)
    print('Logos Storage proxy on :8080 → upstream :8888')
    s.serve_forever()
