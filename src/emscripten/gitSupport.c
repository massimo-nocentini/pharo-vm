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
 * the CORS proxy of isomorphic-git takes them; a '/' is put after the prefix
 * when it has none, and a prefix that is not an http:// or https:// URL
 * fails the request.  It is Module.gitHttpProxy, which the Web Worker sets
 * from the settings field of the Console page (packaging/emscripten/web/
 * vm-worker.js) and this file reads at each request, or else the
 * environment variable PHARO_WASM_GIT_PROXY (node); there is none by
 * default.  The proxy sees everything that goes through it, the code and
 * any credentials in the remote's URL: it is never taken from the URL of a
 * page.  Without a proxy the request goes to the remote itself, whose
 * response a browser gives only when the remote is of the page's origin or
 * answers CORS.
 *
 * A request whose response the browser does not give (a network error,
 * CORS) fails with 'git http: request failed (CORS or network)', and a
 * response other than 200 with the content type of the service, as
 * libgit2's own transport checks it, fails too.  There are no credentials:
 * a remote that asks for them (401, 403 or 407) fails with GIT_EAUTH.
 */

#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

/* Make the request, synchronously, and answer the HTTP status, or -1 when
 * there is no response (the error is then in Module.pharoGitHttp.error).
 * The response, its body and its content type, waits in Module.pharoGitHttp
 * for gitHttpResponse() and gitHttpTake().  envProxy is the value of
 * PHARO_WASM_GIT_PROXY, or NULL; Module.gitHttpProxy comes first.
 */
EM_JS_DEPS(pharoGitHttp, "$UTF8ToString,$stringToUTF8");
EM_JS(int, gitHttpRequest, (const char *method, const char *url, const char *contentType,
		const char *accept, const unsigned char *body, int bodyLength, const char *envProxy), {
	var state = Module['pharoGitHttp'] = { body: null, type: "", error: "" };
	method = UTF8ToString(Number(method));
	url = UTF8ToString(Number(url));
	contentType = UTF8ToString(Number(contentType));
	accept = UTF8ToString(Number(accept));
	envProxy = Number(envProxy) ? UTF8ToString(Number(envProxy)) : "";
	var data = bodyLength > 0 ? HEAPU8.slice(Number(body), Number(body) + bodyLength) : null;
	var proxy = Module['gitHttpProxy'] || envProxy;
	if (proxy) {
		if (!/^https?:[/][/][^/]/i.test(proxy)) {
			state.error = "git http: the proxy '" + proxy + "' is not an http:// or https:// URL";
			return -1;
		}
		url = proxy + (proxy.endsWith("/") ? "" : "/") + url.replace(/^https?:[/][/]/i, "");
	}
	try {
		if (typeof XMLHttpRequest != "undefined") {
			var request = new XMLHttpRequest();
			request.open(method, url, false);
			request.responseType = "arraybuffer";
			if (contentType)
				request.setRequestHeader("Content-Type", contentType);
			request.setRequestHeader("Accept", accept);
			request.send(data);
			state.body = new Uint8Array(request.response || new ArrayBuffer(0));
			state.type = request.getResponseHeader("Content-Type") || "";
			return request.status || -1;
		}
		if (typeof require != "function")
			throw new Error("no XMLHttpRequest, and no curl");
		/* node: curl writes the body, then the content type and the status
		 * on two lines of their own */
		var args = ["-s", "-L", "-X", method, "-o", "-", "-w", "\n%{content_type}\n%{http_code}",
			"-H", "Accept: " + accept];
		if (contentType)
			args.push("-H", "Content-Type: " + contentType);
		if (data)
			args.push("--data-binary", "@-");
		args.push("--", url);
		var out = require("child_process").execFileSync("curl", args,
			{ input: data ? Buffer.from(data) : undefined, maxBuffer: 1 << 30, stdio: ["pipe", "pipe", "pipe"] });
		var statusLine = out.lastIndexOf(10);
		var typeLine = out.lastIndexOf(10, statusLine - 1);
		state.body = new Uint8Array(out.subarray(0, typeLine));
		state.type = out.subarray(typeLine + 1, statusLine).toString();
		return parseInt(out.subarray(statusLine + 1).toString(), 10) || -1;
	} catch (e) {
		state.body = null;
		/* (execFileSync's error repeats the command: give curl's status) */
		state.error = "git http: request failed (CORS or network): "
			+ (e && typeof e.status == "number" ? "curl exited with status " + e.status : e && e.message ? e.message : e);
		return -1;
	}
});

/* Copy the content type of the response into type (at most size bytes, with
 * its NUL), and answer the length of its body */
EM_JS(int, gitHttpResponse, (char *type, int size), {
	var state = Module['pharoGitHttp'];
	stringToUTF8(state.type, Number(type), size);
	return state.body ? state.body.length : 0;
});

/* Copy the body of the response to destination (unless it is NULL), and
 * forget the response */
EM_JS(void, gitHttpTake, (unsigned char *destination), {
	var state = Module['pharoGitHttp'];
	if (Number(destination) && state.body && state.body.length)
		HEAPU8.set(state.body, Number(destination));
	Module['pharoGitHttp'] = null;
});

/* Copy the error of the request into message (at most size bytes) */
EM_JS(void, gitHttpError, (char *message, int size), {
	var state = Module['pharoGitHttp'];
	stringToUTF8(state && state.error ? state.error : "git http: request failed (CORS or network)", Number(message), size);
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

static int
gitHttpPerform(GitHttpStream *stream)
{
	char message[512], type[128];
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
	if (status < 0) {
		gitHttpError(message, sizeof(message));
		git_error_set_str(GIT_ERROR_NET, message);
		return -1;
	}
	length = gitHttpResponse(type, sizeof(type));
	if (status == 401 || status == 403 || status == 407) {
		gitHttpTake(NULL);
		snprintf(message, sizeof(message), "git http: the remote answered %d: authentication is not supported",
			status);
		git_error_set_str(GIT_ERROR_HTTP, message);
		return GIT_EAUTH;
	}
	if (status != 200) {
		gitHttpTake(NULL);
		snprintf(message, sizeof(message), "git http: unexpected http status code: %d", status);
		git_error_set_str(GIT_ERROR_HTTP, message);
		return -1;
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
gitHttpFree(git_smart_subtransport *subtransport)
{
	free(subtransport);
}

static int
gitHttpSubtransport(git_smart_subtransport **out, git_transport *owner, void *param)
{
	git_smart_subtransport *subtransport = calloc(1, sizeof(*subtransport));

	(void)owner;
	(void)param;
	if (!subtransport) {
		git_error_set_oom();
		return -1;
	}
	subtransport->action = gitHttpAction;
	subtransport->close = gitHttpClose;
	subtransport->free = gitHttpFree;
	*out = subtransport;
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
