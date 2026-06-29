// Swagger UI bootstrap. Kept in a same-origin file (not inline) so the
// per-route Content-Security-Policy can stay `script-src 'self'` without
// the `'unsafe-inline'` escape hatch. See `web/src/api.rs::api_docs`.
window.onload = function () {
  window.ui = SwaggerUIBundle({
    url: "/openapi.json",
    dom_id: "#swagger-ui",
    deepLinking: true,
    presets: [
      SwaggerUIBundle.presets.apis,
      SwaggerUIStandalonePreset.slice(1) // drop the topbar
    ],
    plugins: [SwaggerUIBundle.plugins.DownloadUrl],
    layout: "BaseLayout",
    tryItOutEnabled: true,
    persistAuthorization: false,
    requestInterceptor: function (request) {
      request.headers = request.headers || {};
      request.headers["X-Navigator-Swagger-UI"] = "1";
      return request;
    },
    defaultModelsExpandDepth: 1,
    docExpansion: "list"
  });
};
