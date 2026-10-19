/* gitSupport.c -- libgit2 in the Emscripten (WebAssembly) VM, with
 * WASM_LIBGIT2 (cmake/Emscripten.cmake compiles this file then)
 *
 * libgit2 is linked in (cmake/emscripten/deps/libgit2.cmake), and the image
 * calls it through its own bindings (LGitLibrary, libgit2.so.1.4.4 to the
 * image), which find it in the registry of the libraries of the FFI
 * (cmake/emscripten/ffiLibraries.cmake).  This file gives it:
 *
 *  - __wrap_getaddrinfo, which the links put in place of getaddrinfo
 *    (-Wl,--wrap=getaddrinfo): Emscripten's getaddrinfo aborts a memory64
 *    runtime (a BigInt of a string) instead of failing, and could only make
 *    addresses up anyway.  It answers EAI_FAIL, as the SocketPlugin does
 *    under Emscripten without calling it, so that a git:// remote, or an
 *    http:// one without the transport below, fails with 'failed to resolve
 *    address for <host>', and the VM goes on.  libgit2 is the only caller.
 *
 *  - pharoWasmGitInit, the ON_LOAD function of the library in the registry,
 *    which src/externalPrimitives.c calls the first time the image loads
 *    it.  It initializes libgit2 once: the image initializes it and shuts
 *    it down too, in pairs, and this reference of the VM's own keeps it
 *    initialized between them, with the transports registered here.
 *
 *  - with WASM_LIBGIT2_HTTP (PHARO_WASM_GIT_HTTP), a smart-HTTP subtransport
 *    for the http:// and https:// remotes, registered in place of libgit2's
 *    own, which would need sockets.  Each request of git's smart HTTP
 *    protocol, which is stateless (one request, one response), is made
 *    synchronously from JavaScript, by the browser: a synchronous
 *    XMLHttpRequest in the Web Worker of the VM, which blocks the VM until
 *    the response has come.  The browser does the TLS, and asks for CORS:
 *    git servers answer no CORS headers, so a page of another origin can
 *    read them only through a CORS proxy.
 *    In node (no XMLHttpRequest), the request is made with curl, through
 *    child_process.execFileSync, for the tests.
 *
 * The proxy is a URL prefix, put in place of the scheme of the remote's URLs
 * (https://github.com/o/r.git with the proxy https://proxy.example/ is
 * requested as https://proxy.example/github.com/o/r.git/info/refs?...), as
 * the CORS proxy of isomorphic-git takes them; the prefix is the proxy's
 * URL as the browser writes it (new URL(proxy).href: no default port, the
 * scheme and the host in lower case, as XMLHttpRequest's responseURL has
 * them, see Redirects), with a '/' after it when it has none, and a prefix
 * that is not an http:// or https:// URL fails the request.  It is Module.gitHttpProxy, which the Web Worker sets
 * from what the page gives it (packaging/emscripten/web/vm-worker.js) and
 * this file reads at each request, or else the environment variable
 * PHARO_WASM_GIT_PROXY (node).  The pages give the site's default
 * (manifest.gitProxyDefault, the build's WASM_GIT_PROXY) unless the user
 * chose another proxy, or none, in Settings.  The proxy sees everything that
 * goes through it, the code and the URLs of the repositories: it is never
 * taken from the URL of a page, and the user name and password of a
 * remote's URL (https://user:token@host/...) never go to it (the site's
 * default is a public proxy, which the user did not choose): the request
 * through a proxy is made without them.  Without a proxy the request goes
 * to the remote itself, whose response a browser gives only when the remote
 * is of the page's origin or answers CORS.
 *
 * Redirects.  The browser (or curl -L, which goes from an https:// URL only
 * to another https:// one, as libgit2's own transport does) follows them,
 * and the request remembers where its response came from: XMLHttpRequest's
 * responseURL, or curl's url_effective, less the proxy's prefix (with the
 * remote's scheme put back; a final URL is compared with the prefix as it
 * is, then as the browser writes it, and one outside the prefix is not the
 * remote's, and is left alone).  When the advertisement of the refs (GET .../info/refs?service=)
 * answered 200 from another URL than it asked, one that ends with the same
 * path, and not an https:// remote gone to http://, the subtransport keeps
 * that URL, less the path, as the remote's (from, to), and the next requests
 * of the same remote (the POSTs of the pack) go there, as libgit2's own HTTP
 * transport does: GitLab and Codeberg send a remote named without .git to
 * its .git with a 301, and answer 422 to a POST to the old name.
 *
 * Errors.  A request whose response the browser does not give (CORS, the
 * network, a proxy that is down) fails with 'git http: request failed (CORS
 * or network)...', which says what to do: without a proxy (or with a proxy
 * that is no http(s) URL), choose one in Settings, as GIT_ERROR_HTTP, which
 * Iceberg does not retry (a CORS failure fails again the same); through a
 * proxy, check it, as GIT_ERROR_NET, which Iceberg retries (a proxy may be
 * down for a while).  gitHttpRequest answers -1 and -2 for these.  A
 * response other than 200 fails as GIT_ERROR_HTTP, with a message that
 * names the host, or the proxy, or the URL: 401 (a private or missing
 * repository, as github.com answers both) and 407 (a proxy that asks for
 * credentials) with GIT_EAUTH, 403 (refused by the proxy, or by the host),
 * 404 (no repository there) and any other with -1.  A 200 without the
 * content type of the service, as libgit2's own transport checks it, fails
 * too (a dumb HTTP server, or a page).  The VM sends no credentials, so
 * neither a private repository nor a push to a host that asks for them can
 * work, and there are no SSH or scp-like remotes.
 *
 * Module.onGitHttp, when the page's worker set it, is called at the end of
 * every request with {method, host, proxy, status, error}: the remote's
 * host (and port), the proxy's origin (new URL(proxy).origin, as the pages
 * write it) or '', the status (-1 or -2 without a
 * response) and the error ('' with a response).  The worker turns it into
 * its 'git' messages, which the pages show as notices; what it throws is
 * ignored.
 */

#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

#include <emscripten.h>
#include <git2.h>
#include <git2/sys/transport.h>

#include "pharovm/debug.h"

/* What getaddrinfo answers in the VM: see above */
int __wrap_getaddrinfo(const char *restrict node, const char *restrict service,
	const struct addrinfo *restrict hints, struct addrinfo **restrict result);

int
__wrap_getaddrinfo(const char *restrict node, const char *restrict service,
	const struct addrinfo *restrict hints, struct addrinfo **restrict result)
{
	(void)node;
	(void)service;
	(void)hints;
	*result = NULL;
	return EAI_FAIL;
}

#if PHARO_WASM_GIT_HTTP

/* Make the request, synchronously, and answer the HTTP status, or else -1
 * (no proxy, or one that is no http(s) URL) or -2 (through a proxy) when
 * there is no response; the error is then in Module.pharoGitHttp.error.
 * The response, its body and its content type, waits in Module.pharoGitHttp
 * for gitHttpResponse(), gitHttpFinalUrl(), gitHttpDescribe() and
 * gitHttpTake(), with what they say of it: url, the URL asked for (the
 * remote's, without the proxy), finalUrl, the one the response came from
 * (likewise), host, the remote's host and port, and proxyOrigin, the
 * proxy's origin or "".  envProxy is the value of PHARO_WASM_GIT_PROXY, or
 * NULL; Module.gitHttpProxy comes first.  Module.onGitHttp, if any, is told
 * of the request at the end.
 * (No backslash in the JavaScript of EM_JS but in its strings: the body is
 * made a C string as it is.)
 */
EM_JS_DEPS(pharoGitHttp, "$UTF8ToString,$stringToUTF8,$lengthBytesUTF8");
EM_JS(int, gitHttpRequest, (const char *method, const char *url, const char *contentType,
		const char *accept, const unsigned char *body, int bodyLength, const char *envProxy), {
	method = UTF8ToString(Number(method));
	url = UTF8ToString(Number(url));
	contentType = UTF8ToString(Number(contentType));
	accept = UTF8ToString(Number(accept));
	envProxy = Number(envProxy) ? UTF8ToString(Number(envProxy)) : "";
	var data = bodyLength > 0 ? HEAPU8.slice(Number(body), Number(body) + bodyLength) : null;
	var proxy = Module['gitHttpProxy'] || envProxy;
	/* the scheme and the authority of an http(s) URL, without its user info */
	var parts = function (u) {
		var m = /^(https?:)[/][/]([^/?#]*)/i.exec(u);
		return m ? { scheme: m[1].toLowerCase(), host: m[2].replace(/^.*@/, "") } : null;
	};
	var remote = parts(url);
	/* the URL without its user info, which never goes to a proxy */
	var bare = url.replace(/^(https?:[/][/])[^/?#]*@/i, "$1");
	var state = Module['pharoGitHttp'] = { body: null, type: "", error: "", url: url, finalUrl: "",
		host: remote ? remote.host : "", proxyOrigin: "", withoutUser: false };
	var status = (function () {
		var prefix = "", requested = url;
		/* no response: -1 without a proxy, -2 through one, and what to do */
		var failed = function (detail) {
			state.body = null;
			if (state.proxyOrigin) {
				state.error = "git http: request failed (CORS or network) through the proxy " + state.proxyOrigin
					+ ": the proxy is down or refuses this page, or the network is down; check Settings";
				return -2;
			}
			/* (the browsers end their text with a full stop: one is enough) */
			state.error = "git http: request failed (CORS or network): "
				+ String(detail).slice(0, 300).replace(/[. ]+$/, "")
				+ ". A web page cannot read git servers without a CORS proxy: choose one in Settings (Git HTTP proxy)";
			return -1;
		};
		/* whether u starts with the proxy's prefix */
		var within = function (u) {
			return u.slice(0, prefix.length).toLowerCase() == prefix.toLowerCase();
		};
		/* the URL a response came from, the remote's: without the proxy
		 * (curl may write it as it was asked, the browser as it writes
		 * URLs: the prefix is the latter, see above) */
		var landed = function (final) {
			if (!final)
				return url;
			if (!prefix)
				return final;
			if (!within(final))
				try {
					final = new URL(final).href;
				} catch (e) {
				}
			if (!within(final))
				return url;
			final = (remote ? remote.scheme : "https:") + "//" + final.slice(prefix.length);
			/* (asked without the user info: the same URL) */
			return final == bare ? url : final;
		};
		if (proxy) {
			var normal = null;
			try {
				normal = parts(proxy) && new URL(proxy);
			} catch (e) {
			}
			if (!normal || !/^https?:$/.test(normal.protocol) || !normal.host) {
				state.error = "git http: the proxy '" + proxy + "' is not an http:// or https:// URL";
				return -1;
			}
			state.proxyOrigin = normal.origin;
			prefix = normal.href + (normal.href.endsWith("/") ? "" : "/");
			state.withoutUser = bare != url;
			requested = prefix + bare.replace(/^https?:[/][/]/i, "");
		}
		try {
			if (typeof XMLHttpRequest != "undefined") {
				var request = new XMLHttpRequest();
				request.open(method, requested, false);
				request.responseType = "arraybuffer";
				if (contentType)
					request.setRequestHeader("Content-Type", contentType);
				request.setRequestHeader("Accept", accept);
				request.send(data);
				if (!request.status)
					return failed("no response");
				state.body = new Uint8Array(request.response || new ArrayBuffer(0));
				state.type = request.getResponseHeader("Content-Type") || "";
				state.finalUrl = landed(request.responseURL);
				return request.status;
			}
			if (typeof require != "function")
				throw new Error("no XMLHttpRequest, and no curl");
			/* node: curl writes the body, then the URL it ended at, the
			 * content type and the status on three lines of their own */
			var args = ["-s", "-L", "-X", method, "-o", "-", "-w", "\n%{url_effective}\n%{content_type}\n%{http_code}",
				"-H", "Accept: " + accept];
			/* (an https:// request is never redirected to http://) */
			if (/^https:/i.test(requested))
				args.push("--proto-redir", "=https");
			if (contentType)
				args.push("-H", "Content-Type: " + contentType);
			if (data)
				args.push("--data-binary", "@-");
			args.push("--", requested);
			var out = require("child_process").execFileSync("curl", args,
				{ input: data ? Buffer.from(data) : undefined, maxBuffer: 1 << 30, stdio: ["pipe", "pipe", "pipe"] });
			var statusLine = out.lastIndexOf(10);
			var typeLine = out.lastIndexOf(10, statusLine - 1);
			var urlLine = out.lastIndexOf(10, typeLine - 1);
			var code = parseInt(out.subarray(statusLine + 1).toString(), 10);
			if (!code)
				return failed("no response");
			state.body = new Uint8Array(out.subarray(0, urlLine));
			state.finalUrl = landed(out.subarray(urlLine + 1, typeLine).toString());
			state.type = out.subarray(typeLine + 1, statusLine).toString();
			return code;
		} catch (e) {
			/* (execFileSync's error repeats the command: give curl's status) */
			return failed(e && typeof e.status == "number" ? "curl exited with status " + e.status
				: e && e.message ? e.message : e);
		}
	})();
	if (Module['onGitHttp'])
		try {
			Module['onGitHttp']({ method: method, host: state.host, proxy: state.proxyOrigin, status: status,
				error: state.error });
		} catch (e) {
		}
	return status;
});

/* Copy the content type of the response into type (at most size bytes, with
 * its NUL), and answer the length of its body */
EM_JS(int, gitHttpResponse, (char *type, int size), {
	var state = Module['pharoGitHttp'];
	stringToUTF8(state.type, Number(type), size);
	return state.body ? state.body.length : 0;
});

/* Copy the URL that the response came from, without the proxy, into out (at
 * most size bytes, with its NUL; nothing when size is 0), and answer its
 * length; or answer 0 when it is the URL asked for, or unknown */
EM_JS(int, gitHttpFinalUrl, (char *out, int size), {
	var state = Module['pharoGitHttp'];
	if (!state || !state.finalUrl || state.finalUrl == state.url)
		return 0;
	if (size > 0)
		stringToUTF8(state.finalUrl, Number(out), size);
	return lengthBytesUTF8(state.finalUrl);
});

/* Copy what a response of the given status, other than 200, says into
 * message (at most size bytes): who answered it, and what it means here */
EM_JS(void, gitHttpDescribe, (int status, char *message, int size), {
	var state = Module['pharoGitHttp'] || {};
	var host = state.host || "the git server", origin = state.proxyOrigin || "";
	/* (the URL asked for, without its query, nor any user info) */
	var where = String(state.url || "").replace(/[?#].*$/, "").replace(/^(https?:[/][/])[^/?#]*@/i, "$1");
	var text;
	if (status == 401)
		text = "git http: " + host + " answered 401: the repository does not exist or is private"
			+ (state.withoutUser ? " (this VM sends no credentials: the user name and password of the remote's URL"
				+ " never go through the proxy " + origin + ")" : " (this VM sends no credentials)");
	else if (status == 403 && origin)
		text = "git http: the proxy " + origin + " refused the request to " + host
			+ " (403): it forwards only git requests of public repositories";
	else if (status == 403)
		text = "git http: " + host + " answered 403: access denied (a private repository, or a rate limit)";
	else if (status == 404)
		text = "git http: no git repository at " + where + " (404)";
	else if (status == 407)
		text = "git http: the proxy asks for credentials (407), which this VM does not support";
	else
		text = "git http: unexpected http status code: " + status;
	stringToUTF8(text, Number(message), size);
});

/* Copy the body of the response to destination (unless it is NULL), and
 * forget the response */
EM_JS(void, gitHttpTake, (unsigned char *destination), {
	var state = Module['pharoGitHttp'];
	if (Number(destination) && state.body && state.body.length)
		HEAPU8.set(state.body, Number(destination));
	Module['pharoGitHttp'] = null;
});

/* Copy the error of the request into message (at most size bytes), and
 * forget the request; without an error of its own, the one that
 * gitHttpRequest would have made */
EM_JS(void, gitHttpError, (char *message, int size), {
	var state = Module['pharoGitHttp'];
	var text = state && state.error ? state.error
		: state && state.proxyOrigin
		? "git http: request failed (CORS or network) through the proxy " + state.proxyOrigin
			+ ": the proxy is down or refuses this page, or the network is down; check Settings"
		: "git http: request failed (CORS or network): no response."
			+ " A web page cannot read git servers without a CORS proxy: choose one in Settings (Git HTTP proxy)";
	stringToUTF8(text, Number(message), size);
	Module['pharoGitHttp'] = null;
});

/* The requests of the services: what the URL of the remote is given, and
 * the content types of the request and of the response (libgit2's
 * src/transports/http.c) */
typedef struct {
	const char *path;
	const char *method;
	const char *requestType;	/* "" for a GET */
	const char *responseType;
} GitHttpService;

static const GitHttpService uploadPackList = {
	"/info/refs?service=git-upload-pack", "GET", "",
	"application/x-git-upload-pack-advertisement" };
static const GitHttpService uploadPack = {
	"/git-upload-pack", "POST", "application/x-git-upload-pack-request",
	"application/x-git-upload-pack-result" };
static const GitHttpService receivePackList = {
	"/info/refs?service=git-receive-pack", "GET", "",
	"application/x-git-receive-pack-advertisement" };
static const GitHttpService receivePack = {
	"/git-receive-pack", "POST", "application/x-git-receive-pack-request",
	"application/x-git-receive-pack-result" };

/* The subtransport of a remote: where the advertisement of its refs was
 * redirected to, if it was (see the header): from is the remote's URL, as
 * libgit2 gives it to gitHttpAction, and to the URL its requests go to
 * instead, both without the path of a service */
typedef struct {
	git_smart_subtransport parent;
	char *from, *to;
} GitHttpSubtransport;

/* A request: what libgit2 writes is its body, sent at the first read, which
 * then reads the response */
typedef struct {
	git_smart_subtransport_stream parent;
	const GitHttpService *service;
	char *url;
	unsigned char *request;
	size_t requestLength, requestCapacity;
	unsigned char *response;
	size_t responseLength, position;
	int sent;
} GitHttpStream;

/* After the advertisement of the refs answered 200: when its response came
 * from another URL, which ends with the same path, and is not an https://
 * remote gone to http://, the next requests of the remote go there */
static void
gitHttpRedirected(GitHttpStream *stream)
{
	GitHttpSubtransport *subtransport = (GitHttpSubtransport *)stream->parent.subtransport;
	size_t pathLength = strlen(stream->service->path), urlLength = strlen(stream->url), finalLength;
	int length = gitHttpFinalUrl(NULL, 0);
	char *final, *from, *to;

	if (length <= 0 || !(final = malloc((size_t)length + 1)))
		return;
	gitHttpFinalUrl(final, length + 1);
	finalLength = strlen(final);
	if (strcmp(final, stream->url) == 0
	 || finalLength <= pathLength || urlLength < pathLength
	 || strcmp(final + finalLength - pathLength, stream->service->path) != 0
	 || (strncasecmp(final, "http://", 7) != 0 && strncasecmp(final, "https://", 8) != 0)
	 || (strncasecmp(stream->url, "https://", 8) == 0 && strncasecmp(final, "https://", 8) != 0)) {
		free(final);
		return;
	}
	from = strndup(stream->url, urlLength - pathLength);
	to = strndup(final, finalLength - pathLength);
	free(final);
	if (!from || !to) {
		/* (no memory: the requests go on to the remote's own URL) */
		free(from);
		free(to);
		return;
	}
	free(subtransport->from);
	free(subtransport->to);
	subtransport->from = from;
	subtransport->to = to;
}

static int
gitHttpPerform(GitHttpStream *stream)
{
	char message[1024], type[128];
	const char *envProxy = getenv("PHARO_WASM_GIT_PROXY");
	size_t typeLength;
	int status, length;

	if (stream->requestLength > 0x7FFFFFFF) {
		git_error_set_str(GIT_ERROR_NET, "git http: request too large");
		return -1;
	}
	status = gitHttpRequest(stream->service->method, stream->url, stream->service->requestType,
		stream->service->responseType, stream->request, (int)stream->requestLength,
		envProxy && *envProxy ? envProxy : NULL);
	stream->sent = 1;
	/* No response: without a proxy (-1) the browser's CORS refused it, and
	 * would again, so GIT_ERROR_HTTP, which Iceberg does not retry; through
	 * a proxy (-2), which may be down for a while, GIT_ERROR_NET, which it
	 * retries */
	if (status < 0) {
		gitHttpError(message, sizeof(message));
		git_error_set_str(status == -2 ? GIT_ERROR_NET : GIT_ERROR_HTTP, message);
		return -1;
	}
	length = gitHttpResponse(type, sizeof(type));
	if (status != 200) {
		gitHttpDescribe(status, message, sizeof(message));
		gitHttpTake(NULL);
		git_error_set_str(GIT_ERROR_HTTP, message);
		return status == 401 || status == 407 ? GIT_EAUTH : -1;
	}
	/* The content type, without its parameters, must be the service's:
	 * a dumb HTTP server, or a page, answers something else */
	typeLength = strcspn(type, "; \t");
	if (typeLength != strlen(stream->service->responseType)
	 || strncmp(type, stream->service->responseType, typeLength) != 0) {
		gitHttpTake(NULL);
		snprintf(message, sizeof(message), "git http: invalid content-type: '%s' (not a smart HTTP git server)", type);
		git_error_set_str(GIT_ERROR_HTTP, message);
		return -1;
	}
	if (stream->service == &uploadPackList || stream->service == &receivePackList)
		gitHttpRedirected(stream);
	if (!(stream->response = malloc(length > 0 ? (size_t)length : 1))) {
		gitHttpTake(NULL);
		git_error_set_oom();
		return -1;
	}
	gitHttpTake(stream->response);
	stream->responseLength = (size_t)length;
	return 0;
}

static int
gitHttpRead(git_smart_subtransport_stream *parent, char *buffer, size_t size, size_t *count)
{
	GitHttpStream *stream = (GitHttpStream *)parent;
	size_t left;
	int error;

	*count = 0;
	if (!stream->sent && (error = gitHttpPerform(stream)) < 0)
		return error;
	left = stream->responseLength - stream->position;
	if (size > left)
		size = left;
	if (size)
		memcpy(buffer, stream->response + stream->position, size);
	stream->position += size;
	*count = size;
	return 0;
}

static int
gitHttpWrite(git_smart_subtransport_stream *parent, const char *buffer, size_t length)
{
	GitHttpStream *stream = (GitHttpStream *)parent;

	if (stream->sent) {
		git_error_set_str(GIT_ERROR_NET, "git http: write after the request was sent");
		return -1;
	}
	if (length > stream->requestCapacity - stream->requestLength) {
		size_t capacity = (stream->requestLength + length) * 2;
		unsigned char *request = realloc(stream->request, capacity);

		if (!request) {
			git_error_set_oom();
			return -1;
		}
		stream->request = request;
		stream->requestCapacity = capacity;
	}
	memcpy(stream->request + stream->requestLength, buffer, length);
	stream->requestLength += length;
	return 0;
}

static void
gitHttpFreeStream(git_smart_subtransport_stream *parent)
{
	GitHttpStream *stream = (GitHttpStream *)parent;

	free(stream->url);
	free(stream->request);
	free(stream->response);
	free(stream);
}

static int
gitHttpAction(git_smart_subtransport_stream **out, git_smart_subtransport *subtransport,
	const char *url, git_smart_service_t action)
{
	GitHttpSubtransport *owner = (GitHttpSubtransport *)subtransport;
	const GitHttpService *service;
	GitHttpStream *stream;

	switch (action) {
	case GIT_SERVICE_UPLOADPACK_LS:		service = &uploadPackList;	break;
	case GIT_SERVICE_UPLOADPACK:		service = &uploadPack;		break;
	case GIT_SERVICE_RECEIVEPACK_LS:	service = &receivePackList;	break;
	case GIT_SERVICE_RECEIVEPACK:		service = &receivePack;		break;
	default:
		git_error_set_str(GIT_ERROR_NET, "git http: unknown action");
		return -1;
	}
	/* (the remote's URL, or where it was redirected to) */
	if (owner->from && strcmp(url, owner->from) == 0)
		url = owner->to;
	if (!(stream = calloc(1, sizeof(*stream)))
	 || !(stream->url = malloc(strlen(url) + strlen(service->path) + 1))) {
		free(stream);
		git_error_set_oom();
		return -1;
	}
	strcpy(stream->url, url);
	strcat(stream->url, service->path);
	stream->service = service;
	stream->parent.subtransport = subtransport;
	stream->parent.read = gitHttpRead;
	stream->parent.write = gitHttpWrite;
	stream->parent.free = gitHttpFreeStream;
	*out = &stream->parent;
	return 0;
}

static int
gitHttpClose(git_smart_subtransport *subtransport)
{
	(void)subtransport;
	return 0;
}

static void
gitHttpFree(git_smart_subtransport *parent)
{
	GitHttpSubtransport *subtransport = (GitHttpSubtransport *)parent;

	free(subtransport->from);
	free(subtransport->to);
	free(subtransport);
}

static int
gitHttpSubtransport(git_smart_subtransport **out, git_transport *owner, void *param)
{
	GitHttpSubtransport *subtransport = calloc(1, sizeof(*subtransport));

	(void)owner;
	(void)param;
	if (!subtransport) {
		git_error_set_oom();
		return -1;
	}
	subtransport->parent.action = gitHttpAction;
	subtransport->parent.close = gitHttpClose;
	subtransport->parent.free = gitHttpFree;
	*out = &subtransport->parent;
	return 0;
}

/* Stateless: one request, one response (rpc) */
static git_smart_subtransport_definition gitHttpDefinition = { gitHttpSubtransport, 1, NULL };

static int
gitHttpTransport(git_transport **out, git_remote *owner, void *param)
{
	return git_transport_smart(out, owner, param);
}

#endif /* PHARO_WASM_GIT_HTTP */

/* The ON_LOAD function of libgit2 in the registry of the FFI */
void pharoWasmGitInit(void);

void
pharoWasmGitInit(void)
{
	static int initialized;
	int error;

	if (initialized)
		return;
	initialized = 1;
	if ((error = git_libgit2_init()) < 0) {
		logError("libgit2 could not be initialized (%d)\n", error);
		return;
	}
#if PHARO_WASM_GIT_HTTP
	for (int i = 0; i < 2; i++) {
		const char *scheme = i ? "http" : "https";

		git_transport_unregister(scheme);
		if ((error = git_transport_register(scheme, gitHttpTransport, &gitHttpDefinition)) < 0) {
			const git_error *last = git_error_last();

			logError("The %s transport of libgit2 could not be registered (%d): %s\n",
				scheme, error, last && last->message ? last->message : "");
		}
	}
#endif
}
