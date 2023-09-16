//
//  RustProxy.swift
//  NetworkSpy
//
//  Created by M on 10/09/22.
//

import Foundation
import ProxyRust

private let kDefaultPort: UInt16 = 9090

public typealias ClientListListener = (String) -> Void
public typealias HttpTrafficListener = (Traffic) -> Void

public final class Proxy {
    public init() {
    }
    
    public lazy var port: UInt16 = {
        var port = kDefaultPort
        while !isPortOpen(port: port) {
            port += 1
        }
        return port
    }()
    
    public var clientListListener: ClientListListener?
    public var httpTrafficListener: HttpTrafficListener?
    
    private var currentId: UInt8 = 0
    
    private static var counter: UInt8 = 0
    private static var context: [UInt8:Context] = [:]
    
    private lazy var proxy = proxy_new(
        ProxyArg(
            ip_v4_addr: (0,0,0,0),
            port: port
        )
    )
    deinit {
        proxy_free(proxy)
    }
    
    private final class Context {
        let proxy: Proxy
        let queue: DispatchQueue
        init(proxy: Proxy, queue: DispatchQueue) {
            self.queue = queue
            self.proxy = proxy
        }
    }

    // Platform-specific function to check if a port is open (Linux)
    #if os(Linux)
    private func isPortOpen(port: in_port_t) -> Bool {
        return false;
    }
    #else
    // https://stackoverflow.com/a/65162953
    private func isPortOpen(port: in_port_t) -> Bool {
        
        let socketFileDescriptor = socket(AF_INET, SOCK_STREAM, 0)
        if socketFileDescriptor == -1 {
            return false
        }

        var addr = sockaddr_in()
        let sizeOfSockkAddr = MemoryLayout<sockaddr_in>.size
        addr.sin_len = __uint8_t(sizeOfSockkAddr)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = Int(OSHostByteOrder()) == OSLittleEndian ? _OSSwapInt16(port) : port
        addr.sin_addr = in_addr(s_addr: inet_addr("0.0.0.0"))
        addr.sin_zero = (0, 0, 0, 0, 0, 0, 0, 0)
        var bind_addr = sockaddr()
        memcpy(&bind_addr, &addr, Int(sizeOfSockkAddr))

        if Darwin.bind(socketFileDescriptor, &bind_addr, socklen_t(sizeOfSockkAddr)) == -1 {
            return false
        }
        let isOpen = Darwin.listen(socketFileDescriptor, SOMAXCONN ) != -1
        Darwin.close(socketFileDescriptor)
        return isOpen
    }
    #endif
    
    private typealias RustCallback = @convention(c) (
        UInt8,
        OpaquePointer?
    ) -> Void
    
    public func listen(in queue: DispatchQueue) {
        currentId = Proxy.counter
        DispatchQueue.global(qos: .background).async { [weak self] in
            guard let self = self else { return }
            
            var req_callback: RustCallback? = { id, reqPtr in
                guard let ctx = Proxy.context[id] else { return }
                RustTraffic.request(ptr: reqPtr!) { ip, traffic in
                    ctx.queue.async {
                        ctx.proxy.httpTrafficListener?(traffic)
                        ctx.proxy.clientListListener?(ip)
                    }
                }
            }
            
            var res_callback: RustCallback? = { id, resPtr in
                guard let ctx = Proxy.context[id] else { return }
                RustTraffic.response(ptr: resPtr!) { ip, traffic in
                    ctx.queue.async {
                        ctx.proxy.httpTrafficListener?(traffic)
                        ctx.proxy.clientListListener?(ip)
                    }
                }
            }
            
            let ctx = Context(proxy: self, queue: queue)
            Proxy.context[self.currentId] = ctx
            proxy_listen(
                self.proxy,
                &req_callback,
                &res_callback,
                self.currentId
            )
            Proxy.counter += 1
        }
    }
    
    public func unlisten() {
        proxy_unlisten(
            self.proxy,
            self.currentId
        )
        Proxy.context.removeValue(forKey: currentId)
    }
    
    public func getIPAddress() -> String {
        let ip = get_ip_address()!
        defer { get_ip_address_free(ip) }
        return String(cString: ip)
    }
}

public enum Traffic {
    case httpReq(ip: String, port: UInt16, req: HttpReq)
    case httpRes(ip: String, port: UInt16, res: HttpRes)
}

enum RustTraffic {
    static func request(ptr: OpaquePointer, completion: @escaping (String,Traffic) -> Void) {
        let ip = req_body_http_context_ip(ptr)!;
        defer { req_body_http_context_ip_free(ip) }
        let port = req_body_http_context_port(ptr)
        let ipAddr = String(cString: ip)
        completion(ipAddr,.httpReq(ip: ipAddr, port: port, req: HttpReq.from(ptr)))
        req_body_free(ptr)
    }
    
    static func response(ptr: OpaquePointer, completion: @escaping (String,Traffic) -> Void) {
        let ip = res_body_http_context_ip(ptr)!;
        defer { res_body_http_context_ip_free(ip) }
        let port = res_body_http_context_port(ptr)
        let ipAddr = String(cString: ip)
        completion(ipAddr,.httpRes(ip: ipAddr, port: port, res: HttpRes.from(ptr)))
        res_body_free(ptr)
    }
}

public final class HeaderPair {
    public fileprivate(set) var key: String
    public fileprivate(set) var value: String
    init(_ p: (String, String)) {
        self.key = p.0
        self.value = p.1
    }
}

public struct HttpReq {
    public let url: String
    
    public let version: String
    
    public let listHeaders: [HeaderPair]
    
    public let methodString: String
    
    public let body: Data?
    
    static func from(_ ptr: OpaquePointer) -> HttpReq {
        let uri = req_body_http_uri(ptr)!
        defer { req_body_http_uri_free(uri) }
        let url = String(cString: uri)
        
        let version: String = {
            let ffi_val = req_body_http_version(ptr)!
            defer { req_body_http_version_free(ffi_val) }
            return String(cString: ffi_val)
        }()
        
        let _headers = req_body_http_headers(ptr)!
        defer { req_body_http_headers_free(_headers) }
        let headersStr = String(cString: _headers)
        let headers: [HeaderPair] = headersStr.split(separator: "\r\n").map { pair in
            let p = pair.split(separator: ":")
            let key = String(p.first ?? "")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            let value = String(p.count == 2 ? p[1] : "")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return HeaderPair((key,value))
        }
        
        let method = req_body_http_method(ptr)!
        defer { req_body_http_method_free(method) }
        let methodString = String(cString: method)
        
        let bodySize = req_body_http_body_len(ptr)
        let bodyMut = UnsafeMutablePointer<UInt8>.allocate(capacity: Int(bodySize))
        defer { bodyMut.deallocate() }
        req_body_http_write_body(ptr, bodyMut)
        let body = Data(bytes: UnsafeRawPointer(bodyMut), count: Int(bodySize))
        
        return HttpReq(
            url: url,
            version: version,
            listHeaders: headers,
            methodString: methodString,
            body: body
        )
    }
}

public struct HttpRes {
    public let httpVersion: String
    
    public let status: Int
    
    public let time: Double
    
    public let listHeaders: [HeaderPair]
    
    public let body: Data
    
    static func from(_ ptr: OpaquePointer) -> HttpRes {
        let status = res_body_http_status(ptr)
        
        let version: String = {
            let ffi_val = res_body_http_version(ptr)!
            defer { res_body_http_version_free(ffi_val) }
            return String(cString: ffi_val)
        }()
        
        let _headers = res_body_http_headers(ptr)!
        defer { res_body_http_headers_free(_headers) }
        let headersStr = String(cString: _headers)
        let headers: [HeaderPair] = headersStr.split(separator: "\r\n").map { pair in
            let p = pair.split(separator: ":")
            let key = String(p.first ?? "")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            let value = String(p.count == 2 ? p[1] : "")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return HeaderPair((key,value))
        }
        
        let bodySize = res_body_http_body_len(ptr)
        let bodyMut = UnsafeMutablePointer<UInt8>.allocate(capacity: Int(bodySize))
        defer { bodyMut.deallocate() }
        res_body_http_write_body(ptr, bodyMut)
        let body = Data(bytes: UnsafeRawPointer(bodyMut), count: Int(bodySize))
        
        return HttpRes(
            httpVersion: version,
            status: Int(status),
            time: 0,
            listHeaders: headers,
            body: body
        )
    }
}
