"""Ollama-shaped front for a llama-server started with engine.rs's arguments.

The headless `lipik --clean` starts a fresh process per clip, so its built-in
engine is never loaded and clean-up falls back to the rules. Pointing the
portable copy's Ollama setting here lets the real 0.1.12 pipeline (rules,
gate, prompt, checks) call the 0.1.12 llama-server with the same body the
built-in path sends (engine::chat_body).

    python3 cli_shim.py <listen port> <llama-server url> <api key> <log.jsonl>
"""

import json
import sys
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT, UPSTREAM, KEY, LOG = int(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
MODEL = "lipik-cleanup"


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def send(self, code, obj):
        data = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/api/ps":
            self.send(200, {"models": [{"name": MODEL, "model": MODEL, "context_length": 4096}]})
        elif self.path == "/api/tags":
            self.send(200, {"models": [{"name": MODEL, "model": MODEL}]})
        else:
            self.send(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/api/chat":
            return self.send(404, {"error": "not found"})
        req = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        body = {"messages": req["messages"], "stream": False, "temperature": 0.2, "cache_prompt": True,
                "chat_template_kwargs": {"enable_thinking": False}}
        n = req.get("options", {}).get("num_predict")
        if n is not None:
            body["max_tokens"] = n
        t = time.perf_counter()
        r = urllib.request.Request(f"{UPSTREAM}/v1/chat/completions", data=json.dumps(body).encode(),
                                   headers={"Content-Type": "application/json", "Authorization": f"Bearer {KEY}"})
        with urllib.request.urlopen(r, timeout=60) as resp:
            v = json.loads(resp.read())
        ms = (time.perf_counter() - t) * 1000
        choice = v["choices"][0]
        with open(LOG, "a") as f:
            f.write(json.dumps({"ms": round(ms), "in": req["messages"][-1]["content"],
                                "out": choice["message"]["content"], "finish": choice.get("finish_reason")}) + "\n")
        self.send(200, {"model": MODEL, "message": {"role": "assistant", "content": choice["message"]["content"]},
                        "done": True, "done_reason": "length" if choice.get("finish_reason") == "length" else "stop"})


ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
