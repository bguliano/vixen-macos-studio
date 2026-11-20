"""Call Vixen Studio's MCP tools from a Python script, with no MCP client of your own.

    from vixen_studio import Studio

    with Studio("/path/to/show-copy", helper="/path/to/Vixen.app/Contents/Helpers/vixen-studio-mcp") as studio:
        seq = studio.call("sequence_create", lengthMs=30000)
        studio.call("effect_add", sequenceId=seq["sequenceId"], ...)

Each Studio runs the bridge with --persist: it attaches to the running app's live session when the app has the profile
open, otherwise to the background session every script and bridge on the profile shares (started on first use, it stops
after 10 minutes with no clients). Unsaved documents and their sequence IDs survive between scripts while that session
runs; save to keep them. The helper path may instead come from the VIXEN_STUDIO_MCP environment variable. Standard
library only; copy this file next to your scripts.
"""

import json
import os
import subprocess

class ToolError(Exception):
    """A tool refused the call; the message is the tool's own."""


class Studio:
    def __init__(self, profile, helper=None):
        self.profile = os.path.abspath(profile)
        self.helper = helper or os.environ.get("VIXEN_STUDIO_MCP")
        if not self.helper:
            raise ValueError("Pass helper= or set VIXEN_STUDIO_MCP to the app's Contents/Helpers/vixen-studio-mcp")
        self._process = None
        self._next = 0

    def call(self, tool, arguments=None, **kwargs):
        """Runs one tool and returns its parsed JSON result (or its text when it isn't JSON). Raises ToolError."""
        arguments = dict(arguments or {}, **kwargs)
        try:
            reply = self._request("tools/call", {"name": tool, "arguments": arguments})
        except (BrokenPipeError, ConnectionError):
            # The bridge or its session went away; the documents live in the session, so reconnect once.
            self.close()
            reply = self._request("tools/call", {"name": tool, "arguments": arguments})
        if "error" in reply:
            raise ToolError(reply["error"].get("message", "Request failed"))
        text = reply["result"]["content"][0]["text"]
        if reply["result"].get("isError"):
            raise ToolError(text)
        try:
            return json.loads(text)
        except ValueError:
            return text

    def tools(self):
        """The tools/list entries: names, descriptions and input schemas."""
        return self._request("tools/list", {})["result"]["tools"]

    def close(self):
        if self._process:
            self._process.stdin.close()
            self._process.wait()
            self._process = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def _request(self, method, params):
        if not self._process:
            self._process = subprocess.Popen(
                [self.helper, "--profile", self.profile, "--persist"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            self._send("initialize", {"protocolVersion": "2024-11-05",
                                      "clientInfo": {"name": "vixen_studio.py", "version": "1"},
                                      "capabilities": {}})
            self._process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        return self._send(method, params)

    def _send(self, method, params):
        self._next += 1
        self._process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self._next, "method": method,
                                              "params": params}) + "\n")
        self._process.stdin.flush()
        while True:
            line = self._process.stdout.readline()
            if not line:
                raise ConnectionError("The Vixen Studio session closed the connection")
            reply = json.loads(line)
            if reply.get("id") == self._next:
                return reply
