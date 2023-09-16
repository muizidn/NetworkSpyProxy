
// https://www.sanfoundry.com/c-program-get-ip-address/

#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <netinet/in.h>
#include <net/if.h>
#include <unistd.h>
#include <arpa/inet.h>
 
char * get_ip_addr()
{
    int n;
    struct ifreq ifr;
    char array[] = "en1";
    n = socket(AF_INET, SOCK_DGRAM, 0);
    ifr.ifr_addr.sa_family = AF_INET;
    strncpy(ifr.ifr_name , array , IFNAMSIZ - 1);
    ioctl(n, SIOCGIFADDR, &ifr);
    close(n);
    return inet_ntoa(( (struct sockaddr_in *)&ifr.ifr_addr )->sin_addr);
}

void free_ip_addr(char * ip) 
{
    free(ip);
} 