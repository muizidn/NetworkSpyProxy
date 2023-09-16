#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct Proxy Proxy;

typedef struct ReqBody ReqBody;

typedef struct ResBody ResBody;

typedef struct ProxyArg {
  uint8_t ip_v4_addr[4];
  uint16_t port;
} ProxyArg;

void req_body_free(struct ReqBody *ptr);

void res_body_free(struct ResBody *ptr);

char *req_body_http_context_ip(struct ReqBody *ptr);

void req_body_http_context_ip_free(char *ptr);

uint16_t req_body_http_context_port(struct ReqBody *ptr);

char *req_body_http_uri(struct ReqBody *ptr);

void req_body_http_uri_free(char *ptr);

char *req_body_http_method(struct ReqBody *ptr);

void req_body_http_method_free(char *ptr);

char *req_body_http_headers(struct ReqBody *ptr);

void req_body_http_headers_free(char *ptr);

char *req_body_http_version(struct ReqBody *ptr);

void req_body_http_version_free(char *ptr);

uintptr_t req_body_http_body_len(struct ReqBody *ptr);

void req_body_http_write_body(struct ReqBody *ptr, uint8_t *data);

char *res_body_http_context_ip(struct ResBody *ptr);

void res_body_http_context_ip_free(char *ptr);

uint16_t res_body_http_context_port(struct ResBody *ptr);

uint16_t res_body_http_status(struct ResBody *ptr);

char *res_body_http_version(struct ResBody *ptr);

void res_body_http_version_free(char *ptr);

char *res_body_http_headers(struct ResBody *ptr);

void res_body_http_headers_free(char *ptr);

uintptr_t res_body_http_body_len(struct ResBody *ptr);

void res_body_http_write_body(struct ResBody *ptr, uint8_t *data);

struct Proxy *proxy_new(struct ProxyArg arg);

void proxy_listen(struct Proxy *ptr,
                  void (**req_callback)(uint8_t, struct ReqBody*),
                  void (**res_callback)(uint8_t, struct ResBody*),
                  uint8_t id);

void proxy_unlisten(struct Proxy *ptr, uint8_t id);

void proxy_free(struct Proxy *ptr);

char *get_ip_address(void);

void get_ip_address_free(char *ptr);
