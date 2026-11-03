"""Read-only IPv4 router discovery. Does not open ports or change router settings."""

import argparse
import ipaddress
import json
import socket
import struct
import time
import urllib.request
import urllib.parse
import xml.etree.ElementTree as ET


def local_url(url):
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "http" or not parsed.hostname:
        raise ValueError("Expected a local HTTP router endpoint")
    address = ipaddress.ip_address(parsed.hostname)
    if not address.is_private or address.is_loopback or address.is_unspecified:
        raise ValueError("Router endpoint must be a private LAN address")
    return url


def read_url(url, data=None, headers=None):
    request = urllib.request.Request(local_url(url), data=data, headers=headers or {})
    # Router traffic should not be sent through a configured HTTP proxy.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(request, timeout=4) as response:
        body = response.read(1_048_577)
        if len(body) > 1_048_576:
            raise ValueError("Router response exceeds 1 MB")
        return ET.fromstring(body)


def discover(local_ip):
    locations = set()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind((local_ip, 0))
        sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(local_ip))
        sock.settimeout(0.5)
        for target in ("urn:schemas-upnp-org:device:InternetGatewayDevice:1", "urn:schemas-upnp-org:device:InternetGatewayDevice:2", "ssdp:all"):
            message = ('M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n'
                       'MAN: "ssdp:discover"\r\nMX: 2\r\n'
                       f'ST: {target}\r\n\r\n')
            sock.sendto(message.encode("ascii"), ("239.255.255.250", 1900))
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            try:
                data, _ = sock.recvfrom(65535)
            except socket.timeout:
                continue
            for line in data.decode("utf-8", errors="replace").splitlines():
                key, _, value = line.partition(":")
                if key.lower() == "location":
                    locations.add(value.strip())
    return sorted(locations)[:10]


def upnp_probe(location):
    document = read_url(location)
    namespace = {"d": "urn:schemas-upnp-org:device-1-0"}
    base = document.findtext("d:URLBase", namespaces=namespace) or location
    results = []
    for service in document.findall(".//d:service", namespace):
        kind = service.findtext("d:serviceType", "", namespace)
        if ":WANIPConnection:" not in kind and ":WANPPPConnection:" not in kind:
            continue
        control = urllib.parse.urljoin(base, service.findtext("d:controlURL", "", namespace))
        body = (f'<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" '
                f's:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">'
                f'<s:Body><u:GetExternalIPAddress xmlns:u="{kind}"/>'
                f'</s:Body></s:Envelope>').encode("utf-8")
        response = read_url(control, body, {
            "Content-Type": 'text/xml; charset="utf-8"',
            "SOAPAction": f'"{kind}#GetExternalIPAddress"',
        })
        external_ip = next((e.text for e in response.iter() if e.tag.split("}")[-1] == "NewExternalIPAddress"), None)
        results.append({"service": kind, "control_url": control, "wan_ip": external_ip})
    return results


def natpmp_probe(local_ip, gateway):
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind((local_ip, 0))
        sock.connect((gateway, 5351))
        sock.settimeout(3)
        sock.send(b"\x00\x00")  # Public address request, no port mapping.
        data = sock.recv(64)
        if len(data) != 12:
            raise ValueError("Unexpected NAT-PMP response length")
        version, opcode, result, epoch, address = struct.unpack("!BBHI4s", data)
        if version != 0 or opcode != 128 or result != 0:
            raise ValueError(f"NAT-PMP returned result {result}")
        return {"wan_ip": socket.inet_ntoa(address), "epoch_seconds": epoch}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local-ip", required=True)
    parser.add_argument("--gateway", required=True)
    args = parser.parse_args()
    report = {"local_ip": args.local_ip, "gateway": args.gateway, "upnp": []}
    try:
        locations = discover(args.local_ip)
        report["ssdp_locations"] = locations
        for location in locations:
            try:
                report["upnp"].extend(upnp_probe(location))
            except Exception as error:
                report.setdefault("discovery_errors", []).append(str(error))
    except Exception as error:
        report["ssdp_error"] = str(error)
    try:
        report["natpmp"] = natpmp_probe(args.local_ip, args.gateway)
    except Exception as error:
        report["natpmp_error"] = str(error)
    print(json.dumps(report, ensure_ascii=False, indent=2))
