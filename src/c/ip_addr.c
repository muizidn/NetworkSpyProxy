#include <stdlib.h>
#include <string.h>

#ifdef _WIN32

#include <winsock2.h>
#include <iphlpapi.h>
#include <ws2tcpip.h>

#pragma comment(lib, "iphlpapi.lib")
#pragma comment(lib, "ws2_32.lib")

char *get_ip_addr() {
    IP_ADAPTER_ADDRESSES *addresses = NULL;
    IP_ADAPTER_ADDRESSES *addr = NULL;
    IP_ADAPTER_UNICAST_ADDRESS *unicast = NULL;

    ULONG size = 15000;

    addresses = (IP_ADAPTER_ADDRESSES *)malloc(size);
    if (!addresses) return NULL;

    if (GetAdaptersAddresses(AF_INET, 0, NULL, addresses, &size) != NO_ERROR) {
        free(addresses);
        return NULL;
    }

    for (addr = addresses; addr != NULL; addr = addr->Next) {
        for (unicast = addr->FirstUnicastAddress; unicast != NULL; unicast = unicast->Next) {

            SOCKADDR_IN *sa = (SOCKADDR_IN *)unicast->Address.lpSockaddr;

            if (sa->sin_addr.S_un.S_addr == htonl(INADDR_LOOPBACK))
                continue;

            char *ip = malloc(INET_ADDRSTRLEN);
            if (!ip) {
                free(addresses);
                return NULL;
            }

            inet_ntop(AF_INET, &(sa->sin_addr), ip, INET_ADDRSTRLEN);

            free(addresses);
            return ip;
        }
    }

    free(addresses);
    return NULL;
}

#else

#include <ifaddrs.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <net/if.h>

char *get_ip_addr() {
    struct ifaddrs *ifaddr = NULL;
    struct ifaddrs *ifa = NULL;

    if (getifaddrs(&ifaddr) == -1)
        return NULL;

    for (ifa = ifaddr; ifa != NULL; ifa = ifa->ifa_next) {

        if (!ifa->ifa_addr)
            continue;

        if (ifa->ifa_addr->sa_family == AF_INET) {

            struct sockaddr_in *sa = (struct sockaddr_in *)ifa->ifa_addr;

            if (ntohl(sa->sin_addr.s_addr) == INADDR_LOOPBACK)
                continue;

            char *ip = malloc(INET_ADDRSTRLEN);
            if (!ip) {
                freeifaddrs(ifaddr);
                return NULL;
            }

            inet_ntop(AF_INET, &(sa->sin_addr), ip, INET_ADDRSTRLEN);

            freeifaddrs(ifaddr);
            return ip;
        }
    }

    freeifaddrs(ifaddr);
    return NULL;
}

#endif

void free_ip_addr(char *ip) {
    if (ip)
        free(ip);
}