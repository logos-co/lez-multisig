#!/usr/bin/env python3
"""Minimal mock of Logos Storage (Codex) API for demo purposes.
POST /api/codex/v1/data  → stores content (handles multipart/raw), returns fake CID
GET  /api/codex/v1/data/<cid>/network/stream → returns stored content
"""
from http.server import HTTPServer, BaseHTTPRequestHandler
import hashlib, json, io, cgi

store = {}

def extract_file_from_multipart(content_type, body):
    environ = {
        'REQUEST_METHOD': 'POST',
        'CONTENT_TYPE': content_type,
        'CONTENT_LENGTH': str(len(body)),
    }
    fs = cgi.FieldStorage(fp=io.BytesIO(body), environ=environ, keep_blank_values=True)
    for key in fs.keys():
        field = fs[key]
        if hasattr(field, 'file') and field.file:
            return field.file.read()
    return body

class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args): pass

    def do_POST(self):
        if self.path == '/api/codex/v1/data':
            length = int(self.headers.get('Content-Length', 0))
            raw = self.rfile.read(length)
            content_type = self.headers.get('Content-Type', '')
            data = extract_file_from_multipart(content_type, raw) if 'multipart/form-data' in content_type else raw
            cid = 'bafy' + hashlib.sha256(data).hexdigest()[:40]
            store[cid] = data
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps({'cid': cid}).encode())
        else:
            self.send_response(404); self.end_headers()

    def do_GET(self):
        if self.path.startswith('/api/codex/v1/data/'):
            parts = self.path.split('/')
            cid = parts[5] if len(parts) > 5 else ''
            if cid and cid in store:
                self.send_response(200)
                self.send_header('Content-Type', 'application/octet-stream')
                self.end_headers()
                self.wfile.write(store[cid])
            else:
                self.send_response(404); self.end_headers()
        else:
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')

if __name__ == '__main__':
    s = HTTPServer(('127.0.0.1', 8080), Handler)
    print('Mock Codex running on :8080')
    s.serve_forever()
