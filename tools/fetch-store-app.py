#!/usr/bin/env python3
"""Fetch Microsoft Store packages (MSIX/MSIXBundle + framework dependencies) anonymously.

Reimplements the request flow a Windows Store client uses, as documented by the
open-source StoreListings/Raven projects (endpoints and SOAP shapes verified
live 2026-10-17):

  1. storeedgefd.dsx.mp.microsoft.com  v9.0 products/{id}   -> WuCategoryId
     (fallback: displaycatalog.mp.microsoft.com v7.0/products/{id})
  2. fe3cr.delivery.mp.microsoft.com   GetCookie            -> anonymous cookie
  3. fe3cr.delivery.mp.microsoft.com   SyncUpdates          -> package + framework updates
  4. fe3cr.../client.asmx/secured      GetExtendedUpdateInfo2 -> time-limited dl.delivery URLs

Every downloaded file is verified against the digest the service itself reports
(base64 in the update fragment — SHA-1 in practice, SHA-256 also accepted). The
computed SHA-256 is recorded in the local manifest as the durable pin. Download
URLs carry expiring signatures, so they are recorded only in the manifest,
never pinned in docs/artifacts.tsv.

Usage:
  tools/fetch-store-app.py 9N7R5S6B0ZZH [product-id ...] [--out artifacts/store] [--list]

Stdlib only. No credentials: the whole flow is the anonymous (no-Microsoft-account)
path — the WindowsUpdateTicketsToken is sent empty.
"""

import argparse
import base64
import hashlib
import json
import os
import re
import ssl
import sys
import time
import urllib.error
import urllib.request
import uuid
import xml.etree.ElementTree as ET

WU_NS = "http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService"
SOAP_NS = "http://www.w3.org/2003/05/soap-envelope"
FE3_URL = "https://fe3cr.delivery.mp.microsoft.com/ClientWebService/client.asmx"
FE3_SECURED_URL = FE3_URL + "/secured"
STORE_UA = "WindowsStore/22512.1401.1101.0"
WU_UA = "Windows-Update-Agent/10.0.10011.16384 Client-Protocol/2.1"

# Static "installed non-leaf update IDs" a real client reports; copied from the
# StoreListings reference implementation — the service expects a plausible list.
BASE_NONLEAF_IDS = [
    1, 2, 3, 10, 11, 17, 19, 2359974, 2359977, 5143990, 5169043, 5169044,
    5169047, 8788830, 8806526, 9125350, 9154769, 10809856, 23110993, 23110994,
    23110995, 23110996, 23110999, 23111000, 23111001, 23111002, 23111003,
    23111004, 24513870, 28880263, 30077688, 30486944, 59830006, 59830007,
    59830008, 60484010, 62450018, 62450019, 62450020, 98959022, 98959023,
    98959024, 98959025, 98959026, 105939029, 105995585, 106017178, 107825194,
    117765322, 129905029, 130040030, 130040031, 130040032, 130040033,
    133399034, 138372035, 138372036, 139536037, 139536038, 139536039,
    139536040, 142045136, 158941041, 158941042, 158941043, 158941044,
    159776047, 160733048, 160733049, 160733050, 160733051, 160733055,
    160733056, 161870057, 161870058, 161870059,
]


# The fe3cr.delivery.mp.microsoft.com endpoint uses Microsoft's private Windows-Update
# PKI (leaf -> "Microsoft ECC Update Secure Server CA 2.1" -> "Microsoft ECC Product
# Root Certificate Authority 2018"), which is not in public CA bundles — real Windows
# clients trust it via the OS store, and the reference Store clients simply disable
# verification off-Windows. We pin the intermediate instead: the leaf must chain to
# this exact Microsoft CA and match the hostname. Public certificate, published at
# http://www.microsoft.com/pkioops/. Expires 2033-09-28.
PINNED_FE3_CA_PEM = """-----BEGIN CERTIFICATE-----
MIIEYjCCA+igAwIBAgITMwAAAASh9bWIPT8AIgAAAAAABDAKBggqhkjOPQQDAzCB
lDELMAkGA1UEBhMCVVMxEzARBgNVBAgTCldhc2hpbmd0b24xEDAOBgNVBAcTB1Jl
ZG1vbmQxHjAcBgNVBAoTFU1pY3Jvc29mdCBDb3Jwb3JhdGlvbjE+MDwGA1UEAxM1
TWljcm9zb2Z0IEVDQyBQcm9kdWN0IFJvb3QgQ2VydGlmaWNhdGUgQXV0aG9yaXR5
IDIwMTgwHhcNMTgwOTI4MjEzNDIwWhcNMzMwOTI4MjE0NDIwWjCBiDELMAkGA1UE
BhMCVVMxEzARBgNVBAgTCldhc2hpbmd0b24xEDAOBgNVBAcTB1JlZG1vbmQxHjAc
BgNVBAoTFU1pY3Jvc29mdCBDb3Jwb3JhdGlvbjEyMDAGA1UEAxMpTWljcm9zb2Z0
IEVDQyBVcGRhdGUgU2VjdXJlIFNlcnZlciBDQSAyLjEwdjAQBgcqhkjOPQIBBgUr
gQQAIgNiAAT9OT77VaoVe/kkF//JYg4ecyNmzNAYR+ra7lrEUnLImpL17URMD3ZI
SAWZJRmmCUamcVxfrXuRJXc2wYMnAmOQu4j7ze0C6AwotcsbKK7/B3NYhyv/Y9Ez
TmE37hV8JUCjggIEMIICADAOBgNVHQ8BAf8EBAMCAYYwEAYJKwYBBAGCNxUBBAMC
AQAwHQYDVR0OBBYEFBZBsQfHi/PSBhSQJgrbsSvARGLDMFUGA1UdIAROMEwwSgYE
VR0gADBCMEAGCCsGAQUFBwIBFjRodHRwOi8vd3d3Lm1pY3Jvc29mdC5jb20vcGtp
b3BzL0RvY3MvUmVwb3NpdG9yeS5odG0AMBMGA1UdJQQMMAoGCCsGAQUFBwMBMBkG
CSsGAQQBgjcUAgQMHgoAUwB1AGIAQwBBMA8GA1UdEwEB/wQFMAMBAf8wHwYDVR0j
BBgwFoAUQ+9wh7idv+yIGdzGxGt1DXU0MwgwegYDVR0fBHMwcTBvoG2ga4ZpaHR0
cDovL3d3dy5taWNyb3NvZnQuY29tL3BraW9wcy9jcmwvTWljcm9zb2Z0JTIwRUND
JTIwUHJvZHVjdCUyMFJvb3QlMjBDZXJ0aWZpY2F0ZSUyMEF1dGhvcml0eSUyMDIw
MTguY3JsMIGHBggrBgEFBQcBAQR7MHkwdwYIKwYBBQUHMAKGa2h0dHA6Ly93d3cu
bWljcm9zb2Z0LmNvbS9wa2lvcHMvY2VydHMvTWljcm9zb2Z0JTIwRUNDJTIwUHJv
ZHVjdCUyMFJvb3QlMjBDZXJ0aWZpY2F0ZSUyMEF1dGhvcml0eSUyMDIwMTguY3J0
MAoGCCqGSM49BAMDA2gAMGUCMD1rHjP2KzG2WB8lj37wdcOm2dPKNDy0YAQ4uSWb
o9RCHVzY6ISZMHau7HFsYRUkDQIxAJ1NrsRkVSJ0qr4y8wJ0QfY5LR4ibp/nX/nh
Rt0ZZxCmacm3e/Q/CP1zOzzbQWdQug==
-----END CERTIFICATE-----"""


def wu(tag):
    return f"{{{WU_NS}}}{tag}"


def soap(tag):
    return f"{{{SOAP_NS}}}{tag}"


def now_iso():
    return time.strftime("%Y-%m-%dT%H:%M:%S") + ".000Z"


def http_request(req, timeout=60, ctx=None):
    try:
        return urllib.request.urlopen(req, timeout=timeout, context=ctx)
    except urllib.error.HTTPError as e:
        body = e.read(2000).decode("utf-8", "replace")
        raise SystemExit(f"HTTP {e.code} from {req.full_url}\n{body[:1500]}")
    except urllib.error.URLError as e:
        # dl.delivery.mp.microsoft.com may also serve the private WU PKI; the payload
        # is SHA-256-verified against the digest from the (pinned) FE3 response anyway.
        if isinstance(e.reason, ssl.SSLCertVerificationError) and "delivery.mp.microsoft.com" in req.host:
            fallback = ssl._create_unverified_context()
            return urllib.request.urlopen(req, timeout=timeout, context=fallback)
        raise


def get_json(url):
    req = urllib.request.Request(url, headers={
        "User-Agent": STORE_UA,
        "Accept": "*/*",
        "OSIsGenuine": "True",
        "OSIsSMode": "False",
    })
    with http_request(req) as r:
        return json.load(r)


_fe3_ctx = None


def fe3_ssl_context():
    global _fe3_ctx
    if _fe3_ctx is None:
        _fe3_ctx = ssl.create_default_context(cadata=PINNED_FE3_CA_PEM)
    return _fe3_ctx


def post_soap(url, envelope):
    req = urllib.request.Request(url, data=envelope.encode("utf-8"), headers={
        "User-Agent": WU_UA,
        "Content-Type": "application/soap+xml",
    })
    with http_request(req, timeout=120, ctx=fe3_ssl_context()) as r:
        return ET.fromstring(r.read())


# ---------------------------------------------------------------- catalog ----

def get_wu_category_id(product_id, market, lang):
    """WuCategoryId + display metadata from StoreEdgeFD (v9.0)."""
    url = (f"https://storeedgefd.dsx.mp.microsoft.com/v9.0/products/{product_id}"
           f"?market={market}&locale={lang}-{market}&deviceFamily=Windows.Desktop&architecture=x64")
    d = get_json(url)["Payload"]
    title = d.get("Title", product_id)
    for sku in d.get("Skus", []):
        fd = sku.get("FulfillmentData")
        if not fd:
            continue
        fd = json.loads(fd) if isinstance(fd, str) else fd
        if fd.get("WuCategoryId"):
            return fd["WuCategoryId"], title, fd.get("PackageFamilyName", "")
    raise SystemExit(f"no WuCategoryId found for {product_id}")


def get_wu_category_id_displaycatalog(product_id, market, lang):
    """Fallback: displaycatalog v7.0 (accepts the small product id directly)."""
    url = (f"https://displaycatalog.mp.microsoft.com/v7.0/products/{product_id}"
           f"?market={market}&languages={lang}-{market},{lang},neutral")
    d = get_json(url)["Product"]
    title = (d.get("LocalizedProperties") or [{}])[0].get("ProductTitle", product_id)
    for dsa in d.get("DisplaySkuAvailabilities", []):
        fd = dsa.get("Sku", {}).get("Properties", {}).get("FulfillmentData") or {}
        if fd.get("WuCategoryId"):
            return fd["WuCategoryId"], title, fd.get("PackageFamilyName", "")
    raise SystemExit(f"no WuCategoryId found for {product_id} via displaycatalog")


# ------------------------------------------------------------------- SOAP ----

def get_cookie():
    envelope = f"""<Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns="http://www.w3.org/2003/05/soap-envelope">
  <Header>
    <Action d3p1:mustUnderstand="1" xmlns:d3p1="http://www.w3.org/2003/05/soap-envelope" xmlns="http://www.w3.org/2005/08/addressing">http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService/GetCookie</Action>
    <MessageID xmlns="http://www.w3.org/2005/08/addressing">urn:uuid:{uuid.uuid4()}</MessageID>
    <To d3p1:mustUnderstand="1" xmlns:d3p1="http://www.w3.org/2003/05/soap-envelope" xmlns="http://www.w3.org/2005/08/addressing">{FE3_URL}</To>
    <Security d3p1:mustUnderstand="1" xmlns:d3p1="http://www.w3.org/2003/05/soap-envelope" xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd">
      <Timestamp xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd">
        <Created>{now_iso()}</Created><Expires>2044-08-02T20:09:03Z</Expires>
      </Timestamp>
      <WindowsUpdateTicketsToken d4p1:id="ClientMSA" xmlns:d4p1="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd" xmlns="http://schemas.microsoft.com/msus/2014/10/WindowsUpdateAuthorization"></WindowsUpdateTicketsToken>
    </Security>
  </Header>
  <Body>
    <GetCookie xmlns="http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService">
      <oldCookie><Expiration>2016-07-27T07:18:09Z</Expiration></oldCookie>
      <lastChange>2015-10-21T17:01:07.1472913Z</lastChange>
      <currentTime>{now_iso()}</currentTime>
      <protocolVersion>2.50</protocolVersion>
    </GetCookie>
  </Body>
</Envelope>"""
    doc = post_soap(FE3_URL, envelope)
    result = doc.find(soap("Body")).find(wu("GetCookieResponse")).find(wu("GetCookieResult"))
    return {
        "data": result.find(wu("EncryptedData")).text,
        "expiration": result.find(wu("Expiration")).text,
    }


def device_attributes(o):
    flight_enabled = 0 if o["flight_ring"] == "Retail" else 1
    return (
        "BranchReadinessLevel=CB;CurrentBranch={branch};OEMModel=Virtual Machine;"
        "FlightRing={ring};AttrDataVer=21;SystemManufacturer=Microsoft Corporation;"
        "InstallLanguage={lang}-{market};OSUILocale={lang}-{market};InstallationType=Client;"
        "FlightingBranchName={branch};FirmwareVersion=Hyper-V UEFI Release v2.5;"
        "SystemProductName=Virtual Machine;OSSkuId=48;FlightContent=Mainline;App=WU_STORE;"
        "OEMName_Uncleaned=Microsoft Corporation;AppVer=0.0.0.0;OSArchitecture={arch};"
        "SystemSKU=None;UpdateManagementGroup=2;IsFlightingEnabled={flight};"
        "IsDeviceRetailDemo=0;TelemetryLevel=3;OSVersion={osver};DeviceFamily=Windows.{family};"
    ).format(branch=o["current_branch"], ring=o["flight_ring"], lang=o["lang"],
             market=o["market"], arch=o["arch"], flight=flight_enabled,
             osver=o["os_version"], family=o["family"])


def sync_updates_round(cookie, wu_category_id, found_ids, o):
    nonleaf = "".join(f"<int>{i}</int>" for i in BASE_NONLEAF_IDS + found_ids)
    cached = "".join(f"<int>{i}</int>" for i in found_ids)
    envelope = f"""<s:Envelope xmlns:a="http://www.w3.org/2005/08/addressing" xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Header>
    <a:Action s:mustUnderstand="1">http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService/SyncUpdates</a:Action>
    <a:MessageID>urn:uuid:{uuid.uuid4()}</a:MessageID>
    <a:To s:mustUnderstand="1">{FE3_URL}</a:To>
    <o:Security s:mustUnderstand="1" xmlns:o="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd">
      <Timestamp xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd">
        <Created>{now_iso()}</Created><Expires>2044-08-02T20:09:03Z</Expires>
      </Timestamp>
      <wuws:WindowsUpdateTicketsToken wsu:id="ClientMSA" xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd" xmlns:wuws="http://schemas.microsoft.com/msus/2014/10/WindowsUpdateAuthorization"></wuws:WindowsUpdateTicketsToken>
    </o:Security>
  </s:Header>
  <s:Body>
    <SyncUpdates xmlns="http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService">
      <cookie><Expiration>{cookie["expiration"]}</Expiration><EncryptedData>{cookie["data"]}</EncryptedData></cookie>
      <parameters>
        <ExpressQuery>false</ExpressQuery>
        <InstalledNonLeafUpdateIDs>{nonleaf}</InstalledNonLeafUpdateIDs>
        <OtherCachedUpdateIDs>{cached}</OtherCachedUpdateIDs>
        <SkipSoftwareSync>false</SkipSoftwareSync>
        <NeedTwoGroupOutOfScopeUpdates>true</NeedTwoGroupOutOfScopeUpdates>
        <FilterAppCategoryIds><CategoryIdentifier><Id>{wu_category_id}</Id></CategoryIdentifier></FilterAppCategoryIds>
        <TreatAppCategoryIdsAsInstalled>true</TreatAppCategoryIdsAsInstalled>
        <AlsoPerformRegularSync>false</AlsoPerformRegularSync>
        <ComputerSpec />
        <ExtendedUpdateInfoParameters>
          <XmlUpdateFragmentTypes>
            <XmlUpdateFragmentType>Extended</XmlUpdateFragmentType>
            <XmlUpdateFragmentType>Published</XmlUpdateFragmentType>
            <XmlUpdateFragmentType>Core</XmlUpdateFragmentType>
          </XmlUpdateFragmentTypes>
          <Locales><string>{o["lang"]}-{o["market"]}</string><string>{o["lang"]}</string></Locales>
        </ExtendedUpdateInfoParameters>
        <ClientPreferredLanguages><string>{o["lang"]}-{o["market"]}</string></ClientPreferredLanguages>
        <ProductsParameters>
          <SyncCurrentVersionOnly>false</SyncCurrentVersionOnly>
          <DeviceAttributes>{device_attributes(o)}</DeviceAttributes>
          <CallerAttributes>Interactive=1;IsSeeker=0;</CallerAttributes>
          <Products />
        </ProductsParameters>
      </parameters>
    </SyncUpdates>
  </s:Body>
</s:Envelope>"""
    if os.environ.get("STORE_FETCH_DEBUG_REQ"):
        with open(os.environ["STORE_FETCH_DEBUG_REQ"], "w") as f:
            f.write(envelope)
    doc = post_soap(FE3_URL, envelope)
    body = doc.find(soap("Body"))
    if os.environ.get("STORE_FETCH_DEBUG"):
        with open(os.environ["STORE_FETCH_DEBUG"], "wb") as f:
            f.write(ET.tostring(doc))
    result = body.find(wu("SyncUpdatesResponse")).find(wu("SyncUpdatesResult"))
    nc = result.find(wu("NewCookie"))
    new_cookie = {"data": nc.find(wu("EncryptedData")).text,
                  "expiration": nc.find(wu("Expiration")).text}
    extended, new_updates = {}, {}
    ext_info = result.find(wu("ExtendedUpdateInfo"))
    if ext_info is not None:
        upd_list = ext_info.find(wu("Updates"))
        if upd_list is not None:
            for upd in upd_list.findall(wu("Update")):
                extended[upd.find(wu("ID")).text] = upd.find(wu("Xml")).text or ""
    # When nothing is new the server omits NewUpdates entirely.
    nu = result.find(wu("NewUpdates"))
    if nu is not None:
        for ui in nu.findall(wu("UpdateInfo")):
            new_updates[ui.find(wu("ID")).text] = ui.find(wu("Xml")).text or ""
    return new_cookie, extended, new_updates


def parse_version(s):
    try:
        parts = [int(x) for x in s.split(".")]
    except ValueError:
        return (0, 0, 0, 0)
    while len(parts) < 4:
        parts.append(0)
    return tuple(parts[:4])


def version_from_ulong(v):
    return ((v >> 48) & 0xFFFF, (v >> 40) & 0xFF, (v >> 16) & 0xFFFFFF, v & 0xFFFF)


def collect_updates(cookie, wu_category_id, o):
    """Loop SyncUpdates until no more extended updates; parse leaves."""
    found = []
    rounds = []
    while True:
        cookie, extended, new_updates = sync_updates_round(cookie, wu_category_id, found, o)
        if not extended:
            break
        found.extend(extended.keys())
        rounds.append((extended, new_updates))
        if len(found) > 5000:  # runaway guard
            break

    updates = []
    for extended, new_updates in rounds:
        for uid, core in new_updates.items():
            if "<BundledUpdates" in core:
                continue  # bundle parent: relationship tree only, no payload
            ext = extended.get(uid, "")
            if "<Files" not in ext or "SecuredFragment" not in core:
                continue
            try:
                doc = ET.fromstring(f"<Xml>{ext}{core}</Xml>")
            except ET.ParseError:
                continue
            ident = doc.find("UpdateIdentity")
            extprops = doc.find("ExtendedProperties")
            if ident is None or extprops is None:
                continue
            pkgid = extprops.get("PackageIdentityName", "")
            is_fw = extprops.get("IsAppxFramework", "false") == "true"
            platforms = []
            ab = doc.find("ApplicabilityRules/Metadata/AppxPackageMetadata/AppxMetadata/ApplicabilityBlob")
            if ab is not None and ab.text:
                try:
                    blob = json.loads(ab.text)
                    for tp in blob.get("content.targetPlatforms", []):
                        platforms.append((tp.get("platform.target", -1),
                                          version_from_ulong(tp.get("platform.minVersion", 0))))
                except (ValueError, TypeError):
                    pass
            files = doc.find("Files")
            if files is None:
                continue
            for f in files.findall("File"):
                name = f.get("FileName", "")
                if name.endswith(".cab"):
                    continue
                identifier = f.get("InstallerSpecificIdentifier")
                if not identifier:
                    continue
                parts = identifier.split("_")
                updates.append({
                    "update_id": ident.get("UpdateID"),
                    "revision": ident.get("RevisionNumber"),
                    "pkgid": pkgid,
                    "framework": is_fw,
                    "filename": identifier + name[name.rindex("."):],
                    "digest_b64": f.get("Digest"),
                    "version": parts[1] if len(parts) > 1 else "0.0.0.0",
                    "arch": parts[2] if len(parts) > 2 else "neutral",
                    "platforms": platforms,
                })
    return cookie, updates


def get_download_url(cookie, upd, o):
    envelope = f"""<s:Envelope xmlns:a="http://www.w3.org/2005/08/addressing" xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Header>
    <a:Action s:mustUnderstand="1">http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService/GetExtendedUpdateInfo2</a:Action>
    <a:MessageID>urn:uuid:{uuid.uuid4()}</a:MessageID>
    <a:To s:mustUnderstand="1">{FE3_SECURED_URL}</a:To>
    <o:Security s:mustUnderstand="1" xmlns:o="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd">
      <Timestamp xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd">
        <Created>{now_iso()}</Created><Expires>2044-08-02T20:09:03Z</Expires>
      </Timestamp>
      <wuws:WindowsUpdateTicketsToken wsu:id="ClientMSA" xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd" xmlns:wuws="http://schemas.microsoft.com/msus/2014/10/WindowsUpdateAuthorization"></wuws:WindowsUpdateTicketsToken>
    </o:Security>
  </s:Header>
  <s:Body>
    <GetExtendedUpdateInfo2 xmlns="http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService">
      <updateIDs><UpdateIdentity><UpdateID>{upd["update_id"]}</UpdateID><RevisionNumber>{upd["revision"]}</RevisionNumber></UpdateIdentity></updateIDs>
      <infoTypes><XmlUpdateFragmentType>FileUrl</XmlUpdateFragmentType><XmlUpdateFragmentType>FileDecryption</XmlUpdateFragmentType></infoTypes>
      <deviceAttributes>{device_attributes(o)}</deviceAttributes>
    </GetExtendedUpdateInfo2>
  </s:Body>
</s:Envelope>"""
    doc = post_soap(FE3_SECURED_URL, envelope)
    result = (doc.find(soap("Body")).find(wu("GetExtendedUpdateInfo2Response"))
              .find(wu("GetExtendedUpdateInfo2Result")))
    want = upd["digest_b64"]
    for loc in result.find(wu("FileLocations")).findall(wu("FileLocation")):
        if loc.find(wu("FileDigest")).text == want:
            return loc.find(wu("Url")).text
    raise SystemExit(f"no download URL matching digest for {upd['filename']}")


# -------------------------------------------------------------- selection ----

def resource_only(filename):
    n = filename.lower()
    return any(s in n for s in ("language-", "_language", "scale-", "_scale", "resource"))


def family_key(pkgid):
    return ".".join(s for s in pkgid.lower().split(".") if not s.isdigit())


def platform_ok(upd, os_ver):
    if not upd["platforms"]:
        return True
    return any(fam in (0, 3) and minv <= os_ver for fam, minv in upd["platforms"])


def select(updates, o):
    """([main candidates newest-first], [framework deps]) the way the reference
    client does: newest main at the best arch, then per framework family the
    newest at the best arch. A bundle named "neutral" can still contain only one
    architecture (e.g. MyASUS 4.4.x ships arm64-only), so callers must verify
    the downloaded bundle with bundle_arches() and fall back to the next
    candidate — the metadata carries no architecture hint for bundles."""
    arch_priority = ["x64", "neutral", "x86"] if o["arch"] == "AMD64" else ["neutral"]
    os_ver = parse_version(o["os_version"])
    eligible = [u for u in updates if platform_ok(u, os_ver)]
    mains = [u for u in eligible if not u["framework"] and not resource_only(u["filename"])
             and u["arch"] in arch_priority]
    mains.sort(key=lambda u: (arch_priority.index(u["arch"]), [-x for x in parse_version(u["version"])]))
    deps = []
    fw_groups = {}
    for u in eligible:
        if u["framework"] and not resource_only(u["filename"]) and u["arch"] in arch_priority:
            fw_groups.setdefault(family_key(u["pkgid"]), []).append(u)
    for group in fw_groups.values():
        group.sort(key=lambda u: (arch_priority.index(u["arch"]), [-x for x in parse_version(u["version"])]))
        best_arch = group[0]["arch"]
        deps.append(next(u for u in group if u["arch"] == best_arch
                         and parse_version(u["version"]) == parse_version(group[0]["version"])))
    return mains, deps


def bundle_arches(path):
    """Set of Architecture values in a bundle manifest, or None if path is not
    an .msixbundle/.appxbundle (plain .appx carries its arch in the filename)."""
    if not path.endswith((".msixbundle", ".appxbundle")):
        return None
    import zipfile
    try:
        with zipfile.ZipFile(path) as z:
            xml = z.read("AppxMetadata/AppxBundleManifest.xml").decode("utf-8", "replace")
    except Exception:
        return None
    return set(re.findall(r'Architecture="([^"]+)"', xml))


# ------------------------------------------------------------- download ------

def _hash_file(path):
    h1, h256 = hashlib.sha1(), hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h1.update(chunk)
            h256.update(chunk)
    return h1.hexdigest(), h256.hexdigest()


def download(url, dest, digest_b64):
    # The WU service attests File@Digest / FileLocation@FileDigest as base64
    # SHA-1 (20 bytes); some paths carry SHA-256 (32). Verify whichever it is,
    # and always record the SHA-256 we compute ourselves as the local pin.
    want = base64.b64decode(digest_b64).hex() if digest_b64 else None
    algo = {20: "sha1", 32: "sha256"}.get(len(want) // 2 if want else 0, "sha1")
    if os.path.exists(dest):
        got1, got256 = _hash_file(dest)
        if want and (got1 if algo == "sha1" else got256) == want:
            print(f"  already present, digest ok: {os.path.basename(dest)}")
            return got256
    req = urllib.request.Request(url, headers={"User-Agent": STORE_UA})
    tmp = dest + ".part"
    h1, h256 = hashlib.sha1(), hashlib.sha256()
    with http_request(req, timeout=300) as r, open(tmp, "wb") as f:
        total = int(r.headers.get("Content-Length") or 0)
        done = 0
        for chunk in iter(lambda: r.read(1 << 20), b""):
            f.write(chunk)
            h1.update(chunk)
            h256.update(chunk)
            done += len(chunk)
            if total:
                print(f"\r  {os.path.basename(dest)}: {done}/{total} bytes", end="", flush=True)
    print()
    got1, got256 = h1.hexdigest(), h256.hexdigest()
    if want and (got1 if algo == "sha1" else got256) != want:
        os.remove(tmp)
        raise SystemExit(f"DIGEST MISMATCH for {dest}: got sha1={got1} sha256={got256}, "
                         f"want {algo}={want}")
    os.replace(tmp, dest)
    return got256


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("products", nargs="+", help="Store product ids, e.g. 9N7R5S6B0ZZH")
    ap.add_argument("--out", default="artifacts/store")
    ap.add_argument("--market", default="US")
    ap.add_argument("--lang", default="en")
    ap.add_argument("--os-version", default="10.0.26200.8873",
                    help="Windows version reported in device attributes (default: our 25H2 ESD build)")
    ap.add_argument("--arch", default="AMD64", choices=["AMD64", "ARM64", "X86"])
    ap.add_argument("--family", default="Desktop")
    ap.add_argument("--flight-ring", default="Retail")
    ap.add_argument("--current-branch", default="ge_release")
    ap.add_argument("--list", action="store_true", help="resolve and list packages, download nothing")
    args = ap.parse_args()

    o = {"market": args.market, "lang": args.lang, "os_version": args.os_version,
         "arch": args.arch, "family": args.family, "flight_ring": args.flight_ring,
         "current_branch": args.current_branch}

    cookie = get_cookie()
    manifest_path = os.path.join(args.out, "manifest.json")
    manifest = {"generated": time.strftime("%Y-%m-%dT%H:%M:%SZ"), "products": []}
    if os.path.exists(manifest_path):
        with open(manifest_path) as f:
            manifest = json.load(f)

    for pid in args.products:
        try:
            wu_cat, title, pkgfamily = get_wu_category_id(pid, args.market, args.lang)
        except SystemExit:
            wu_cat, title, pkgfamily = get_wu_category_id_displaycatalog(pid, args.market, args.lang)
        print(f"\n=== {title} ({pid}) category {wu_cat}")
        cookie, updates = collect_updates(cookie, wu_cat, o)
        mains, deps = select(updates, o)
        if not mains:
            print(f"  no installable main package found among {len(updates)} updates")
            continue
        if args.list:
            for u in [mains[0]] + deps:
                kind = "framework" if u["framework"] else "MAIN"
                print(f"  {kind:9s} {u['filename']}  v{u['version']} [{u['arch']}]")
            continue
        host_arch = {"AMD64": "x64", "ARM64": "arm64", "X86": "x86"}[o["arch"]]
        main_upd = None
        entries = []
        for cand in mains:
            url = get_download_url(cookie, cand, o)
            dest = os.path.join(args.out, cand["filename"])
            print(f"  fetching {cand['filename']}")
            sha = download(url, dest, cand["digest_b64"])
            arches = bundle_arches(dest)
            if arches is not None and host_arch not in arches:
                print(f"  bundle holds only {sorted(arches)}; not {host_arch} — "
                      f"discarding, trying next candidate")
                os.remove(dest)
                continue
            entries.append({"file": cand["filename"], "sha256": sha, "version": cand["version"],
                            "framework": False, "update_id": cand["update_id"]})
            main_upd = cand
            break
        if main_upd is None:
            print(f"  no {host_arch}-compatible main bundle among {len(mains)} candidates")
            continue
        for u in deps:
            url = get_download_url(cookie, u, o)
            dest = os.path.join(args.out, u["filename"])
            print(f"  fetching {u['filename']}")
            sha = download(url, dest, u["digest_b64"])
            entries.append({"file": u["filename"], "sha256": sha, "version": u["version"],
                            "framework": True, "update_id": u["update_id"]})
        manifest["products"] = [p for p in manifest["products"] if p["product_id"] != pid]
        manifest["products"].append({"product_id": pid, "title": title,
                                     "package_family": pkgfamily, "wu_category_id": wu_cat,
                                     "main_version": main_upd["version"], "files": entries,
                                     "fetched": time.strftime("%Y-%m-%dT%H:%M:%SZ")})
        os.makedirs(args.out, exist_ok=True)
        with open(manifest_path, "w") as f:
            json.dump(manifest, f, indent=2)
        print(f"  main version: {main_upd['version']} → manifest updated")


if __name__ == "__main__":
    main()
