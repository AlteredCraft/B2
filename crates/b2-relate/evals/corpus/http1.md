---
type: note
title: HTTP/1.1
---
The version of HTTP that carries one request and response at a time over a TCP connection and processes them in order. A slow response blocks everything queued behind it — head-of-line blocking — which clients work around by opening several parallel connections to the same server.
