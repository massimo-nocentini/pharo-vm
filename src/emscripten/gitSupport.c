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
 * goes through it, the code and the URLs of the repositories, and the
 * credentials of the requests that carry some (see Credentials): it is never
 * taken from the URL of a page.  Without a proxy the request goes to the
 * remote itself, whose response a browser gives only when the remote is of
 * the page's origin or answers CORS.  The services are git-upload-pack
 * (clone, fetch, pull) and git-receive-pack (push), each an advertisement of
 * the refs (GET .../info/refs?service=) then a POST of the pack: the whole
 * body of a POST is made first (libgit2 writes it, the first read sends
 * it), so it goes with its Content-Length, never chunked, and can be sent
 * again as it is.
 *
 * Credentials.  A request goes without them first.  A 401 (github.com
 * answers it for a push without credentials, and for a private or missing
 * repository) is taken as a challenge of HTTP Basic, whatever its
 * WWW-Authenticate (the proxy of isomorphic-git drops that header): the
 * transport finds a user name and a password (or a token), and makes the
 * same request again with 'Authorization: Basic base64(user:password)', as
 * libgit2's own HTTP transport does.  First the user name and password of
 * the remote's URL (https://user:token@host/...), once; then those of
 * libgit2's credentials callback (git_transport_smart_credentials), which
 * Iceberg gives (LGitCredentialsPlaintext, from IceCredentialStore or, in
 * the world, from Iceberg's dialog: packaging/emscripten/st/web-libgit.st
 * and OSWebCredentialsProvider of OSWindow-Web), offered the plaintext type
 * alone (Iceberg would take an SSH key for any other).  Until the callback
 * gives none, or a few rounds: then the 401 fails with GIT_EAUTH, and says
 * whether credentials were refused or none were given.  The credentials,
 * once accepted, go up front with the next requests of the same connection
 * of the remote (the POST of the pack), and never in a URL: the request is
 * always made to the URL without its user info.  They are only for the host
 * of the remote's URL (host[:port], as libgit2 gives it to gitHttpAction):
 * a 401 that another host answered, the one a redirect went to (see
 * Redirects), or one of a request that a redirect of the advertisement sent
 * to another host, fails with GIT_EAUTH and says so, without asking the
 * callback, which would give those of the remote's host (Iceberg looks them
 * up by the remote's URL).  A request with credentials that a redirect takes
 * to another host: curl does not send them there (in node the transport
 * follows those redirects itself, and drops them for another host), nor
 * does the browser across origins; but through a proxy the redirect stays on
 * the proxy's origin (the proxy of isomorphic-git makes any Location a path
 * of its own), so the browser sends them again, and the proxy passes them on
 * to that host: a synchronous XMLHttpRequest cannot refuse a redirect.  Then
 * the transport fails the request with GIT_EAUTH, without its response, and
 * forgets the credentials, and the message names the host that may have got
 * them (Module.onGitHttp: credentials 'redirected').  They are wiped when the
 * connection is closed (git_remote_disconnect; libgit2 closes the
 * subtransport at the start and the end of each connection) and when the
 * remote is freed, and never logged: Module.onGitHttp learns only whether a
 * request carried some.  Through a proxy they are the proxy's to read:
 * Module.gitHttpCredentials, which the worker makes false while the proxy is
 * the site's default (a public proxy that the user did not choose), refuses
 * them before the callback is asked, as does a proxy of plain http:// on
 * another computer (one whose host is not localhost, [::1] nor a numeric
 * 127.x.y.z); the error then says how to choose a proxy for them in
 * Settings.  The same check is made at each request that would carry
 * credentials (the proxy may have changed since they were given): one that
 * fails sends none, and its 401 then fails as above.  In node,
 * Module.gitHttpCredentials is not set, and the proxy, if any, is the user's
 * own (PHARO_WASM_GIT_PROXY).  The browser sets the header with
 * setRequestHeader (a user name and password in the URL, or given to open(),
 * go nowhere across origins); curl reads it from a file of mode 0600, not
 * from its command line.
 *
 * Redirects.  The browser (or curl -L, which goes from an https:// URL only
 * to another https:// one, as libgit2's own transport does; for a request
 * with credentials node follows them itself, the same way, up to 10, see
 * Credentials) follows them,
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
 * down for a while; a request with credentials that fails so through a proxy
 * says too that the proxy may not allow their Authorization header).
 * gitHttpRequest answers -1 and -2 for these.  A
 * response other than 200 fails as GIT_ERROR_HTTP, with a message that
 * names the host that answered (the one a redirect went to, if any), or the
 * proxy, or the URL, and the first line of what the host said (a text/plain
 * body: github.com explains a refused token there; not a page of HTML):
 * 401 (see Credentials) and 407 (a proxy that asks for credentials) with
 * GIT_EAUTH, 403 (refused by the proxy, whose 403 has no body, or by the
 * host: a token without the right to push, say), 404 (no repository there),
 * 413 (a pack too large for the host, or for the proxy) and any other with
 * -1.  A 200 without the content type of the service, as libgit2's own
 * transport checks it, fails too (a dumb HTTP server, or a page).  A push
 * that the host answered 200, but whose report refuses a ref ('ng <ref>
 * <reason>' of report-status, in the side band when there is one: a hook
 * that declined it, a protected branch of github.com, a token that may not
 * change workflows), fails as GIT_ERROR_HTTP with 'git http: <host> refused
 * the push of <ref>: <reason>', and what the host said in the side band
 * ('remote:' lines): libgit2 1.4.4 fails a push only when the pack was not
 * unpacked, and gives a refused ref to a callback (push_update_reference)
 * that Iceberg does not set, so the push would seem to have worked.  There
 * are no SSH or scp-like remotes.
 *
 * Module.onGitHttp, when the page's worker set it, is called at the end of
 * every request with {method, host, proxy, status, error, authorization,
 * retried, credentials, redirectedTo, proxyRefused}: the remote's host (and
 * port), the proxy's origin (new URL(proxy).origin, as the pages write it)
 * or '', the status (-1 or -2 without a response), the error ('' with a
 * response), whether the request carried credentials, whether it was a 401
 * that the transport answers with credentials (the same request follows),
 * 'refused' when credentials were asked for and may not go through the
 * proxy, 'redirected' when a request with credentials went on to another
 * host through the proxy (redirectedTo, that host), else '', and whether a
 * 403 came from the proxy itself.  The worker turns it into its 'git'
 * messages, which the pages show as notices; what it throws is ignored.
 */

#include <ctype.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

#include <emscripten.h>
#include <git2.h>
#include <git2/sys/credential.h>
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
 * for gitHttpResponse(), gitHttpFinalUrl(), gitHttpDescribe(),
 * gitHttpMayAuthenticate(), gitHttpRedirectedCredentials(), gitHttpReport()
 * and gitHttpTake(), with what they say of it: url, the URL asked for (the
 * remote's, without the proxy, nor its user info), finalUrl, the one the
 * response came from (likewise), host and finalHost, their host and port (in
 * lower case), proxyOrigin, the proxy's origin or "", and refusal, why
 * credentials may not go where the request goes, or "" (see
 * gitHttpMayAuthenticate).  The request goes to the URL without its user
 * info, through the proxy or not, with authorization (the value of an
 * Authorization header) unless it is NULL, or refused: then it goes without,
 * and its 401 fails with the refusal.  In node, a request with authorization
 * follows its redirects itself, and drops it for another host than the
 * remote's; in the browser, one that went on to another host through the
 * proxy, which the browser followed with its Authorization, is marked
 * (credentials 'redirected', and redirectedTo): see the header.
 * envProxy is the value of PHARO_WASM_GIT_PROXY, or NULL;
 * Module.gitHttpProxy comes first.
 * (No backslash in the JavaScript of EM_JS but in its strings: the body is
 * made a C string as it is.)
 */
EM_JS_DEPS(pharoGitHttp, "$UTF8ToString,$stringToUTF8,$lengthBytesUTF8");
EM_JS(int, gitHttpRequest, (const char *method, const char *url, const char *contentType,
		const char *accept, const unsigned char *body, int bodyLength, const char *envProxy,
		const char *authorization), {
	method = UTF8ToString(Number(method));
	url = UTF8ToString(Number(url));
	contentType = UTF8ToString(Number(contentType));
	accept = UTF8ToString(Number(accept));
	envProxy = Number(envProxy) ? UTF8ToString(Number(envProxy)) : "";
	authorization = Number(authorization) ? UTF8ToString(Number(authorization)) : "";
	var data = bodyLength > 0 ? HEAPU8.slice(Number(body), Number(body) + bodyLength) : null;
	var proxy = Module['gitHttpProxy'] || envProxy;
	/* the scheme and the authority of an http(s) URL, without its user
	 * info, the authority in lower case and without the default port of
	 * the scheme, as gitHttpHostOf() makes it */
	var parts = function (u) {
		var m = /^(https?:)[/][/]([^/?#]*)/i.exec(u);
		if (!m)
			return null;
		var scheme = m[1].toLowerCase();
		return { scheme: scheme, host: m[2].replace(/^.*@/, "").toLowerCase()
			.replace(scheme == "https:" ? /([^:]):443$/ : /([^:]):80$/, "$1") };
	};
	var remote = parts(url);
	/* the URL without its user info, which is the one requested: the user
	 * name and password go in the Authorization header, if at all */
	var bare = url.replace(/^(https?:[/][/])[^/?#]*@/i, "$1");
	var state = Module['pharoGitHttp'] = { body: null, type: "", error: "", url: bare, finalUrl: "",
		host: remote ? remote.host : "", finalHost: "", proxyOrigin: "", method: method, status: 0,
		authorization: false, credentials: "", redirectedTo: "", refusal: "", reported: false };
	var status = (function () {
		var prefix = "", requested = bare;
		/* no response: -1 without a proxy, -2 through one, and what to do */
		var failed = function (detail) {
			state.body = null;
			if (state.proxyOrigin) {
				state.error = "git http: request failed (CORS or network) through the proxy " + state.proxyOrigin
					+ ": the proxy is down or refuses this page, or the network is down"
					+ (authorization ? ", or the proxy does not allow the Authorization header of the credentials" : "")
					+ "; check Settings";
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
		/* the remote's URL that a URL of a response stands for: without the
		 * proxy (curl may write it as it was asked, the browser as it writes
		 * URLs: the prefix is the latter, see above), or null for one outside
		 * the prefix, which is not the remote's */
		var remoteOf = function (final) {
			if (!prefix)
				return final;
			if (!within(final))
				try {
					final = new URL(final).href;
				} catch (e) {
				}
			if (!within(final))
				return null;
			return (remote ? remote.scheme : "https:") + "//" + final.slice(prefix.length);
		};
		/* the URL a response came from, the remote's (the URL asked for when
		 * it is unknown, or outside the proxy's prefix) */
		var landed = function (final) {
			var r = final ? remoteOf(final) : null;
			return r || bare;
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
			requested = prefix + bare.replace(/^https?:[/][/]/i, "");
		}
		/* Whether credentials may go where the request goes (see the
		 * header): not through the site's default proxy, which the worker
		 * says (Module.gitHttpCredentials false), nor through a proxy of
		 * plain http:// on another computer, where the network would see them
		 * too.  Only a numeric 127.x.y.z, localhost and [::1] are this
		 * computer: a name such as 127.example.org may be anywhere */
		var host = state.host || "the git server", origin = state.proxyOrigin;
		var local = function (o) {
			try {
				var name = new URL(o).hostname.toLowerCase();
				return name == "localhost" || name == "[::1]" || /^127([.][0-9]{1,3}){3}$/.test(name);
			} catch (e) {
				return false;
			}
		};
		if (origin && Module['gitHttpCredentials'] === false)
			state.refusal = "git http: " + host + " asks for a user name and a password or token (401), which this page sends"
				+ " only through a git proxy that you chose: the site's default, the public proxy " + origin
				+ ", would see them. Choose it, or another, as My proxy in Settings (Git HTTP proxy), then try again";
		else if (origin && /^http:/i.test(origin) && !local(origin))
			state.refusal = "git http: " + host + " asks for a user name and a password or token (401), which this VM does not send"
				+ " through the proxy " + origin + " over plain http://: choose an https:// proxy in Settings (Git HTTP proxy)";
		/* (credentials given before, which may not go where this goes now:
		 * none, and the 401 fails with the refusal) */
		if (state.refusal)
			authorization = "";
		state.authorization = !!authorization;
		/* (what a response says of where it came from) */
		var arrived = function (final) {
			state.finalUrl = landed(final);
			var p = parts(state.finalUrl);
			state.finalHost = p ? p.host : state.host;
		};
		try {
			if (typeof XMLHttpRequest != "undefined") {
				var request = new XMLHttpRequest();
				request.open(method, requested, false);
				request.responseType = "arraybuffer";
				if (contentType)
					request.setRequestHeader("Content-Type", contentType);
				request.setRequestHeader("Accept", accept);
				if (authorization)
					request.setRequestHeader("Authorization", authorization);
				request.send(data);
				if (!request.status)
					return failed("no response");
				state.body = new Uint8Array(request.response || new ArrayBuffer(0));
				state.type = request.getResponseHeader("Content-Type") || "";
				arrived(request.responseURL);
				/* (a redirect on the proxy's origin to another host: the
				 * browser sent the Authorization again, and the proxy passed
				 * it on; across origins, without a proxy, it sends none) */
				if (authorization && state.proxyOrigin && state.finalHost != state.host) {
					state.credentials = "redirected";
					state.redirectedTo = state.finalHost;
				}
				return request.status;
			}
			if (typeof require != "function")
				throw new Error("no XMLHttpRequest, and no curl");
			/* node: curl writes the body, then the URL it ended at, the URL
			 * of a redirect it did not follow, the content type and the
			 * status on four lines of their own.  Without credentials it
			 * follows the redirects (-L); with them, this does, up to 10,
			 * with them to the remote's host only (an https:// request never
			 * to http://) */
			var fs = require("fs"), path = require("path"), dir = null, header = "";
			var curl = function (target, credentials) {
				var args = ["-s", "-X", method, "-o", "-",
					"-w", "\n%{url_effective}\n%{redirect_url}\n%{content_type}\n%{http_code}", "-H", "Accept: " + accept];
				if (!authorization) {
					args.push("-L");
					/* (an https:// request is never redirected to http://) */
					if (/^https:/i.test(target))
						args.push("--proto-redir", "=https");
				}
				if (contentType)
					args.push("-H", "Content-Type: " + contentType);
				/* (the credentials in a file of mode 0600, in a directory of
				 * its own, which -H @file reads: not on the command line,
				 * which any user of the machine may see) */
				if (credentials)
					args.push("-H", "@" + header);
				if (data)
					args.push("--data-binary", "@-");
				args.push("--", target);
				var out = require("child_process").execFileSync("curl", args,
					{ input: data ? Buffer.from(data) : undefined, maxBuffer: 1 << 30, stdio: ["pipe", "pipe", "pipe"] });
				var statusLine = out.lastIndexOf(10);
				var typeLine = out.lastIndexOf(10, statusLine - 1);
				var redirectLine = out.lastIndexOf(10, typeLine - 1);
				var urlLine = out.lastIndexOf(10, redirectLine - 1);
				return { code: parseInt(out.subarray(statusLine + 1).toString(), 10),
					body: out.subarray(0, urlLine),
					effective: out.subarray(urlLine + 1, redirectLine).toString(),
					redirect: out.subarray(redirectLine + 1, typeLine).toString(),
					type: out.subarray(typeLine + 1, statusLine).toString() };
			};
			var answer, credentials = !!authorization;
			try {
				if (authorization) {
					dir = fs.mkdtempSync(path.join(require("os").tmpdir(), "pharo-git-"));
					header = path.join(dir, "header");
					fs.writeFileSync(header, "Authorization: " + authorization + "\n", { mode: 384 });
				}
				for (var target = requested, hops = 0; ; hops++) {
					answer = curl(target, credentials);
					if (!authorization || answer.code < 300 || answer.code > 399 || !answer.redirect || hops >= 10
					 || (/^https:/i.test(target) && !/^https:/i.test(answer.redirect)))
						break;
					/* (to another host, or out of the proxy: without them) */
					var next = remoteOf(answer.redirect), p = next ? parts(next) : null;
					if (!p || p.host != state.host)
						credentials = false;
					target = answer.redirect;
				}
			} finally {
				if (dir)
					fs.rmSync(dir, { recursive: true, force: true });
			}
			if (!answer.code)
				return failed("no response");
			state.body = new Uint8Array(answer.body);
			arrived(answer.effective);
			state.type = answer.type;
			return answer.code;
		} catch (e) {
			/* (execFileSync's error repeats the command: give curl's status) */
			return failed(e && typeof e.status == "number" ? "curl exited with status " + e.status
				: e && e.message ? e.message : e);
		}
	})();
	state.status = status;
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
 * message (at most size bytes): who answered it (the host a redirect went
 * to, if any), and what it means here, then the first line of what the host
 * said, when it said it in plain text (the explanation of github.com, say;
 * not a page of HTML, of a proxy or a CDN) */
EM_JS(void, gitHttpDescribe, (int status, char *message, int size), {
	var state = Module['pharoGitHttp'] || {};
	var host = state.finalHost || state.host || "the git server", origin = state.proxyOrigin || "";
	/* (the URL the response came from, without its query, nor any user info) */
	var where = String(state.finalUrl || state.url || "").replace(/[?#].*$/, "").replace(/^(https?:[/][/])[^/?#]*@/i, "$1");
	var body = state.body || new Uint8Array(0), said = "";
	if (body.length && body.length <= 4096 && /^text[/]plain([ ;]|$)/i.test(state.type || ""))
		try {
			said = new TextDecoder("utf-8").decode(body).split(new RegExp("[\r\n]+")).map(function (l) {
				return l.trim();
			}).filter(function (l) {
				return l;
			})[0] || "";
			said = said.replace(new RegExp("[\u0000-\u001f\u007f]", "g"), " ").slice(0, 200);
		} catch (e) {
			said = "";
		}
	var text;
	if (status == 401 && state.authorization)
		text = "git http: " + host + " refused the user name and password or token (401)";
	else if (status == 401)
		text = "git http: " + host + " answered 401: the repository does not exist, or it is private,"
			+ " or it asks for a user name and a password or token, and none was given"
			+ " (store them for " + host.replace(/:.*$/, "") + " in Iceberg's credentials, then try again)";
	else if (status == 403 && origin && !body.length)
		text = "git http: the proxy " + origin + " refused the request to " + host
			+ " (403): it forwards only the requests of git";
	else if (status == 403 && state.authorization)
		text = "git http: " + host + " answered 403: the user name and token may not do this"
			+ " (a push needs a token with the right to write to the repository)";
	else if (status == 403)
		text = "git http: " + host + " answered 403: access denied (a private repository, or a rate limit)";
	else if (status == 404)
		text = "git http: no git repository at " + where + " (404)";
	else if (status == 407)
		text = "git http: the proxy asks for credentials (407), which this VM does not support";
	else if (status == 413)
		text = "git http: the request is too large for " + (origin ? "the proxy " + origin + " or for " : "") + host
			+ " (413): a push of a pack this large cannot go this way";
	else
		text = "git http: unexpected http status code: " + status;
	if (said && status != 404)
		text += "; " + host + " says: " + said;
	stringToUTF8(text, Number(message), size);
});

/* After a 401: answer 1 when credentials may go where the request went, else
 * copy why not into message (at most size bytes), and answer 0.  Only to the
 * host of the remote's URL, remoteHost (host[:port], in lower case; NULL when
 * unknown, which refuses): not when a redirect sent the request to another
 * host, nor when another host answered the 401, at the end of a redirect;
 * and not through a proxy that gitHttpRequest refused for them (refusal:
 * the site's default, or plain http:// on another computer) */
EM_JS(int, gitHttpMayAuthenticate, (char *message, int size, const char *remoteHost), {
	var state = Module['pharoGitHttp'] || {};
	var host = state.host || "the git server", answered = state.finalHost || host, text = "";
	remoteHost = Number(remoteHost) ? UTF8ToString(Number(remoteHost)) : "";
	if (!remoteHost || host != remoteHost || answered != remoteHost)
		text = "git http: " + (remoteHost || "the remote") + " redirected the request to " + (host != remoteHost ? host : answered)
			+ ", which asks for a user name and a password or token (401): the credentials of "
			+ (remoteHost || "the remote") + " are not sent to another host (use the URL of that host as the remote)";
	else if (state.refusal) {
		text = state.refusal;
		state.credentials = "refused";
	}
	if (!text)
		return 1;
	stringToUTF8(text, Number(message), size);
	return 0;
});

/* After a response: answer 1, and copy the error into message (at most size
 * bytes), when the request carried credentials that the browser sent on to
 * another host, through the proxy (see the header); else answer 0 */
EM_JS(int, gitHttpRedirectedCredentials, (char *message, int size), {
	var state = Module['pharoGitHttp'] || {};
	if (state.credentials != "redirected")
		return 0;
	stringToUTF8("git http: " + state.host + " redirected a request that carried the user name and password or token to "
		+ state.redirectedTo + ", and the browser sent them there again, through the proxy " + state.proxyOrigin
		+ " (a web page cannot stop that): the request is stopped, and the credentials are forgotten. If "
		+ state.redirectedTo + " is not one of yours, revoke that token", Number(message), size);
	return 1;
});

/* Tell Module.onGitHttp of the request, once: retried when it was a 401
 * that the same request with credentials follows (see the header) */
EM_JS(void, gitHttpReport, (int retried), {
	var state = Module['pharoGitHttp'];
	if (!state || state.reported)
		return;
	state.reported = true;
	if (Module['onGitHttp'])
		try {
			Module['onGitHttp']({ method: state.method, host: state.host, proxy: state.proxyOrigin,
				status: state.status, error: state.error, authorization: state.authorization,
				retried: !!retried, credentials: state.credentials, redirectedTo: state.redirectedTo,
				proxyRefused: state.status == 403 && !!state.proxyOrigin && !(state.body && state.body.length) });
		} catch (e) {
		}
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

/* The end of a request with a response: Module.onGitHttp is told of it,
 * then the response is forgotten, its body copied to destination first
 * (unless it is NULL) */
static void
gitHttpEnd(unsigned char *destination, int retried)
{
	gitHttpReport(retried);
	gitHttpTake(destination);
}

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

/* At most this many rounds of credentials for a request (Iceberg's provider
 * gives up at its third): then its 401 fails */
#define GIT_HTTP_AUTH_ROUNDS 5

/* The subtransport of a remote, for a connection (gitHttpClose forgets all
 * of it): where the advertisement of its refs was redirected to, if it was
 * (see the header): from is the remote's URL, as libgit2 gives it to
 * gitHttpAction, and to the URL its requests go to instead, both without the
 * path of a service; and its credentials (see the header): owner, the smart
 * transport, whose callback gives them, remoteHost, the host[:port] of the
 * remote's URL (in lower case), which alone gets them, the user name and the
 * password of the remote's URL (percent-decoded; NULL without them), all
 * taken at the first request of the connection (urlParsed), whether those
 * were tried, and authorization, the value of the Authorization header that
 * the host authorizationHost (host[:port], in lower case: the remote's)
 * accepted, or was last offered (NULL until a 401) */
typedef struct {
	git_smart_subtransport parent;
	char *from, *to;
	git_transport *owner;
	char *remoteHost;
	char *urlUser, *urlPassword;
	int urlParsed, urlCredentialsTried;
	char *authorization, *authorizationHost;
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

/* Wipe a secret, free it, and forget it (the URLs of a remote too, which
 * may have a user name and a password in them) */
static void
gitHttpWipe(char **secret)
{
	if (*secret) {
		memset(*secret, 0, strlen(*secret));
		free(*secret);
		*secret = NULL;
	}
}

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
		gitHttpWipe(&from);
		gitHttpWipe(&to);
		return;
	}
	gitHttpWipe(&subtransport->from);
	gitHttpWipe(&subtransport->to);
	subtransport->from = from;
	subtransport->to = to;
}

/* The authority of an http(s) URL, host[:port] without its user info, in
 * lower case, without the default port of its scheme (as the browser writes
 * it): a new string, or NULL */
static char *
gitHttpHostOf(const char *url)
{
	const char *start, *end, *at;
	char *host;
	size_t length;
	int https;

	if (strncasecmp(url, "http://", 7) == 0)
		start = url + 7, https = 0;
	else if (strncasecmp(url, "https://", 8) == 0)
		start = url + 8, https = 1;
	else
		return NULL;
	end = start + strcspn(start, "/?#");
	for (at = end; at > start && at[-1] != '@'; at--)
		;
	if (at > start)
		start = at;
	if (!(host = strndup(start, (size_t)(end - start))))
		return NULL;
	for (char *c = host; *c; c++)
		*c = (char)tolower((unsigned char)*c);
	length = strlen(host);
	if (https && length > 4 && strcmp(host + length - 4, ":443") == 0 && host[length - 5] != ':')
		host[length - 4] = 0;
	else if (!https && length > 3 && strcmp(host + length - 3, ":80") == 0 && host[length - 4] != ':')
		host[length - 3] = 0;
	return host;
}

/* A new string of the length bytes at from, percent-decoded, or NULL */
static char *
gitHttpUnescape(const char *from, size_t length)
{
	char *out = malloc(length + 1), *o = out;

	if (!out)
		return NULL;
	for (size_t i = 0; i < length; i++) {
		if (from[i] == '%' && i + 2 < length
		 && isxdigit((unsigned char)from[i + 1]) && isxdigit((unsigned char)from[i + 2])) {
			char hex[3] = { from[i + 1], from[i + 2], 0 };

			*o++ = (char)strtol(hex, NULL, 16);
			i += 2;
		} else
			*o++ = from[i];
	}
	*o = 0;
	return out;
}

/* Take the host of the remote's URL, and the user name and the password of
 * its user info (https://user:password@host/...), once a connection */
static void
gitHttpUrlCredentials(GitHttpSubtransport *subtransport, const char *url)
{
	const char *start, *end, *at = NULL, *colon;

	if (subtransport->urlParsed)
		return;
	subtransport->urlParsed = 1;
	subtransport->remoteHost = gitHttpHostOf(url);
	if (strncasecmp(url, "http://", 7) == 0)
		start = url + 7;
	else if (strncasecmp(url, "https://", 8) == 0)
		start = url + 8;
	else
		return;
	end = start + strcspn(start, "/?#");
	for (const char *c = start; c < end; c++)
		if (*c == '@')
			at = c;
	if (!at)
		return;
	colon = memchr(start, ':', (size_t)(at - start));
	subtransport->urlUser = gitHttpUnescape(start, (size_t)((colon ? colon : at) - start));
	if (colon)
		subtransport->urlPassword = gitHttpUnescape(colon + 1, (size_t)(at - colon - 1));
}

/* Forget what the subtransport knows of its connection: the redirect, the
 * remote's host, its credentials (wiped), as libgit2's own http_close does */
static void
gitHttpForget(GitHttpSubtransport *subtransport)
{
	gitHttpWipe(&subtransport->from);
	gitHttpWipe(&subtransport->to);
	free(subtransport->remoteHost);
	subtransport->remoteHost = NULL;
	gitHttpWipe(&subtransport->urlUser);
	gitHttpWipe(&subtransport->urlPassword);
	gitHttpWipe(&subtransport->authorization);
	free(subtransport->authorizationHost);
	subtransport->authorizationHost = NULL;
	subtransport->urlParsed = 0;
	subtransport->urlCredentialsTried = 0;
}

/* base64 of the length bytes at in: a new string, or NULL */
static char *
gitHttpBase64(const unsigned char *in, size_t length)
{
	static const char digits[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
	char *out = malloc((length + 2) / 3 * 4 + 1), *o = out;
	size_t i;

	if (!out)
		return NULL;
	for (i = 0; i + 2 < length; i += 3) {
		*o++ = digits[in[i] >> 2];
		*o++ = digits[((in[i] & 3) << 4) | (in[i + 1] >> 4)];
		*o++ = digits[((in[i + 1] & 15) << 2) | (in[i + 2] >> 6)];
		*o++ = digits[in[i + 2] & 63];
	}
	if (i < length) {
		*o++ = digits[in[i] >> 2];
		if (i + 1 < length) {
			*o++ = digits[((in[i] & 3) << 4) | (in[i + 1] >> 4)];
			*o++ = digits[(in[i + 1] & 15) << 2];
		} else {
			*o++ = digits[(in[i] & 3) << 4];
			*o++ = '=';
		}
		*o++ = '=';
	}
	*o = 0;
	return out;
}

/* Make the Authorization of the subtransport 'Basic base64(user:password)',
 * for host */
static int
gitHttpSetBasic(GitHttpSubtransport *subtransport, const char *host, const char *user, const char *password)
{
	size_t length = strlen(user) + 1 + strlen(password);
	char *pair = malloc(length + 1), *encoded = NULL, *value = NULL, *where = strdup(host);

	if (pair) {
		snprintf(pair, length + 1, "%s:%s", user, password);
		encoded = gitHttpBase64((const unsigned char *)pair, length);
		gitHttpWipe(&pair);
	}
	if (encoded && (value = malloc(strlen("Basic ") + strlen(encoded) + 1))) {
		strcpy(value, "Basic ");
		strcat(value, encoded);
	}
	gitHttpWipe(&encoded);
	gitHttpWipe(&subtransport->authorization);
	free(subtransport->authorizationHost);
	subtransport->authorizationHost = NULL;
	if (!value || !where) {
		gitHttpWipe(&value);
		free(where);
		git_error_set_oom();
		return -1;
	}
	subtransport->authorization = value;
	subtransport->authorizationHost = where;
	return 0;
}

/* After a 401 of a request to host: the credentials of the next try (see
 * the header), and 1; or the error, as gitHttpPerform answers it, the
 * response forgotten */
static int
gitHttpAuthenticate(GitHttpStream *stream, const char *host)
{
	GitHttpSubtransport *subtransport = (GitHttpSubtransport *)stream->parent.subtransport;
	git_credential *credential = NULL;
	char message[1024];
	int error;

	if (!host) {
		gitHttpEnd(NULL, 0);
		git_error_set_oom();
		return -1;
	}
	/* (only for the remote's own host, and through a proxy that may see
	 * them; those of before, if any, go no more) */
	if (!gitHttpMayAuthenticate(message, sizeof(message), subtransport->remoteHost)) {
		gitHttpEnd(NULL, 0);
		gitHttpWipe(&subtransport->authorization);
		git_error_set_str(GIT_ERROR_HTTP, message);
		return GIT_EAUTH;
	}
	/* the user name and password of the remote's URL, once */
	if (subtransport->urlUser && subtransport->urlPassword && !subtransport->urlCredentialsTried) {
		subtransport->urlCredentialsTried = 1;
		if (gitHttpSetBasic(subtransport, host, subtransport->urlUser, subtransport->urlPassword) < 0) {
			gitHttpEnd(NULL, 0);
			return -1;
		}
		gitHttpEnd(NULL, 1);
		return 1;
	}
	/* then the callback's, a user name and a password only */
	error = subtransport->owner
		? git_transport_smart_credentials(&credential, subtransport->owner, subtransport->urlUser,
			GIT_CREDENTIAL_USERPASS_PLAINTEXT)
		: GIT_PASSTHROUGH;
	if (error == 0 && credential && credential->credtype == GIT_CREDENTIAL_USERPASS_PLAINTEXT) {
		git_credential_userpass_plaintext *plain = (git_credential_userpass_plaintext *)credential;

		error = gitHttpSetBasic(subtransport, host, plain->username ? plain->username : "",
			plain->password ? plain->password : "");
		git_credential_free(credential);
		gitHttpEnd(NULL, error < 0 ? 0 : 1);
		return error < 0 ? -1 : 1;
	}
	if (credential)
		git_credential_free(credential);
	/* None (no callback, or one that gave none, or other credentials, or
	 * failed): the 401 fails, and says whether credentials were refused or
	 * none were given (the message of a callback that failed, Iceberg's
	 * 'Invalid username and/or password provided.', says neither) */
	gitHttpDescribe(401, message, sizeof(message));
	gitHttpEnd(NULL, 0);
	gitHttpWipe(&subtransport->authorization);
	git_error_set_str(GIT_ERROR_HTTP, message);
	return GIT_EAUTH;
}

/* The length of the pkt-line at data (left bytes there), its 4 hexadecimal
 * digits included (4 for a flush, 0000), or 0 when there is none */
static size_t
gitHttpPktLength(const unsigned char *data, size_t left)
{
	size_t length = 0;

	if (left < 4)
		return 0;
	for (int i = 0; i < 4; i++) {
		if (!isxdigit(data[i]))
			return 0;
		length = length * 16 + (size_t)(isdigit(data[i]) ? data[i] - '0' : tolower(data[i]) - 'a' + 10);
	}
	if (length == 0)
		return 4;
	return length >= 4 && length <= left ? length : 0;
}

/* Append the length bytes at line, without control characters, to text (at
 * most size bytes, with its NUL), after "; " when it has some already */
static void
gitHttpAppendLine(char *text, size_t size, const unsigned char *line, size_t length)
{
	size_t at = strlen(text);

	if (at && at + 2 < size) {
		strcpy(text + at, "; ");
		at += 2;
	}
	for (size_t i = 0; i < length && at + 1 < size; i++)
		text[at++] = line[i] < 32 || line[i] == 127 ? ' ' : (char)line[i];
	text[at] = 0;
}

/* A line of the report of a push (report-status): keep the ref and the
 * reason of the first 'ng <ref> <reason>' in refused */
static void
gitHttpReportLine(const unsigned char *line, size_t length, char *refused, size_t size)
{
	while (length && (line[length - 1] == '\n' || line[length - 1] == '\r'))
		length--;
	if (*refused || length < 4 || memcmp(line, "ng ", 3) != 0)
		return;
	gitHttpAppendLine(refused, size, line + 3, length - 3);
}

/* What the host said in the side band (band 2, its 'remote:' lines; band 3,
 * an error): its lines, but those of progress (git's 'Resolving deltas:
 * 100% (2/2), done.'), in said */
static void
gitHttpSaid(const unsigned char *text, size_t length, char *said, size_t size)
{
	size_t start = 0;

	for (size_t i = 0; i <= length; i++) {
		if (i < length && text[i] != '\n' && text[i] != '\r')
			continue;
		if (i > start) {
			const unsigned char *line = text + start;
			size_t lineLength = i - start, from = 0;
			int progress = 0;

			for (size_t j = 0; j + 2 < lineLength; j++)
				if (line[j] == '%' && line[j + 1] == ' ' && line[j + 2] == '(')
					progress = 1;
			while (from < lineLength && line[from] == ' ')
				from++;
			if (!progress && from < lineLength)
				gitHttpAppendLine(said, size, line + from, lineLength - from);
		}
		start = i + 1;
	}
}

/* After the POST of a push answered 200, its report, length bytes at data:
 * when the host refused a ref (see the header), copy the error into message
 * (at most size bytes) and answer 1; else answer 0.  The pkt-lines of the
 * report come as they are, or in the side band (each a pkt-line whose first
 * byte is its band: 1 the report, 2 and 3 what the host says), whose band 1
 * is the report in pkt-lines again; a line of the report never starts with
 * the byte 1, 2 or 3 */
static int
gitHttpRefusedPush(const unsigned char *data, size_t length, const char *host, char *message, size_t size)
{
	unsigned char *report = malloc(length ? length : 1);
	size_t reportLength = 0, at, pkt;
	char refused[512] = "", said[512] = "";

	if (!report)
		return 0;
	for (at = 0; at < length && (pkt = gitHttpPktLength(data + at, length - at)) > 0; at += pkt) {
		const unsigned char *payload = data + at + 4;
		size_t payloadLength = pkt - 4;

		if (!payloadLength)
			continue;
		if (payload[0] == 1) {
			memcpy(report + reportLength, payload + 1, payloadLength - 1);
			reportLength += payloadLength - 1;
		} else if (payload[0] == 2 || payload[0] == 3)
			gitHttpSaid(payload + 1, payloadLength - 1, said, sizeof(said));
		else
			gitHttpReportLine(payload, payloadLength, refused, sizeof(refused));
	}
	for (at = 0; at < reportLength && (pkt = gitHttpPktLength(report + at, reportLength - at)) > 0; at += pkt)
		if (pkt > 4)
			gitHttpReportLine(report + at + 4, pkt - 4, refused, sizeof(refused));
	free(report);
	if (!*refused)
		return 0;
	/* (<ref> <reason>: the ref, then the reason, after the first space) */
	{
		char *space = strchr(refused, ' ');

		if (space)
			*space = 0;
		snprintf(message, size, "git http: %s refused the push of %s: %s%s%s%s", host, refused,
			space ? space + 1 : "the push was declined", *said ? "; " : "", *said ? host : "", *said ? " says: " : "");
		if (*said && strlen(message) + 1 < size)
			snprintf(message + strlen(message), size - strlen(message), "%s", said);
	}
	return 1;
}

static int
gitHttpPerform(GitHttpStream *stream)
{
	GitHttpSubtransport *subtransport = (GitHttpSubtransport *)stream->parent.subtransport;
	char message[1024], type[128], *host;
	const char *envProxy = getenv("PHARO_WASM_GIT_PROXY");
	size_t typeLength;
	int status, length, rounds = 0, error;

	if (stream->requestLength > 0x7FFFFFFF) {
		git_error_set_str(GIT_ERROR_NET, "git http: request too large");
		return -1;
	}
	stream->sent = 1;
	host = gitHttpHostOf(stream->url);
	for (;;) {
		/* (the credentials of this host, the remote's, if it asked for
		 * some before) */
		const char *authorization = subtransport->authorization && host && subtransport->authorizationHost
			&& strcmp(host, subtransport->authorizationHost) == 0 ? subtransport->authorization : NULL;

		status = gitHttpRequest(stream->service->method, stream->url, stream->service->requestType,
			stream->service->responseType, stream->request, (int)stream->requestLength,
			envProxy && *envProxy ? envProxy : NULL, authorization);
		/* (credentials that went on to another host: stop, and forget them) */
		if (status > 0 && gitHttpRedirectedCredentials(message, sizeof(message))) {
			gitHttpEnd(NULL, 0);
			gitHttpWipe(&subtransport->authorization);
			free(host);
			git_error_set_str(GIT_ERROR_HTTP, message);
			return GIT_EAUTH;
		}
		/* (the body is all here: the request asked again is the same) */
		if (status != 401)
			break;
		if (++rounds > GIT_HTTP_AUTH_ROUNDS) {
			/* (refused at each round: those of the last go no more) */
			gitHttpWipe(&subtransport->authorization);
			break;
		}
		if ((error = gitHttpAuthenticate(stream, host)) < 0) {
			free(host);
			return error;
		}
	}
	/* No response: without a proxy (-1) the browser's CORS refused it, and
	 * would again, so GIT_ERROR_HTTP, which Iceberg does not retry; through
	 * a proxy (-2), which may be down for a while, GIT_ERROR_NET, which it
	 * retries */
	if (status < 0) {
		free(host);
		gitHttpReport(0);
		gitHttpError(message, sizeof(message));
		git_error_set_str(status == -2 ? GIT_ERROR_NET : GIT_ERROR_HTTP, message);
		return -1;
	}
	length = gitHttpResponse(type, sizeof(type));
	if (status != 200) {
		free(host);
		gitHttpDescribe(status, message, sizeof(message));
		gitHttpEnd(NULL, 0);
		git_error_set_str(GIT_ERROR_HTTP, message);
		return status == 401 || status == 407 ? GIT_EAUTH : -1;
	}
	/* The content type, without its parameters, must be the service's:
	 * a dumb HTTP server, or a page, answers something else */
	typeLength = strcspn(type, "; \t");
	if (typeLength != strlen(stream->service->responseType)
	 || strncmp(type, stream->service->responseType, typeLength) != 0) {
		free(host);
		gitHttpEnd(NULL, 0);
		snprintf(message, sizeof(message), "git http: invalid content-type: '%s' (not a smart HTTP git server)", type);
		git_error_set_str(GIT_ERROR_HTTP, message);
		return -1;
	}
	if (stream->service == &uploadPackList || stream->service == &receivePackList)
		gitHttpRedirected(stream);
	if (!(stream->response = malloc(length > 0 ? (size_t)length : 1))) {
		free(host);
		gitHttpEnd(NULL, 0);
		git_error_set_oom();
		return -1;
	}
	gitHttpEnd(stream->response, 0);
	stream->responseLength = (size_t)length;
	/* (a push that the host refused for a ref fails: see the header) */
	if (stream->service == &receivePack
	 && gitHttpRefusedPush(stream->response, stream->responseLength, host ? host : "the git server",
		message, sizeof(message))) {
		free(host);
		git_error_set_str(GIT_ERROR_HTTP, message);
		return -1;
	}
	free(host);
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

	gitHttpWipe(&stream->url);
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
	/* (the host and the credentials of the remote's URL, from the URL as
	 * libgit2 gives it, before a redirect) */
	gitHttpUrlCredentials(owner, url);
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

/* The end of a connection (git_remote_disconnect), and the start of the
 * next (libgit2 closes the subtransport when it connects): forget it, the
 * credentials too */
static int
gitHttpClose(git_smart_subtransport *subtransport)
{
	gitHttpForget((GitHttpSubtransport *)subtransport);
	return 0;
}

static void
gitHttpFree(git_smart_subtransport *parent)
{
	GitHttpSubtransport *subtransport = (GitHttpSubtransport *)parent;

	gitHttpForget(subtransport);
	free(subtransport);
}

static int
gitHttpSubtransport(git_smart_subtransport **out, git_transport *owner, void *param)
{
	GitHttpSubtransport *subtransport = calloc(1, sizeof(*subtransport));

	(void)param;
	if (!subtransport) {
		git_error_set_oom();
		return -1;
	}
	subtransport->owner = owner;
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
