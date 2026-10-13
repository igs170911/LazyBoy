import json

import pytest

from server import logins as logins_module
from server.logins import Logins, belongs, site_of


def test_cookie_sites_group_by_registrable_domain():
    assert site_of(".shopee.tw") == "shopee.tw"
    assert site_of("www.momoshop.com.tw") == "momoshop.com.tw"
    assert site_of("accounts.google.com") == "google.com"
    assert belongs({"domain": ".seller.shopee.tw"}, {"shopee.tw"}) and not belongs({"domain": "notshopee.tw"}, {"shopee.tw"})


class Computers:
    def __init__(self): self.started = []
    def name(self, id): return "box-" + id
    async def status(self, id, refresh=False): return {"ready": id != "cold"}
    async def start(self, id): self.started.append(id)


async def test_only_chosen_sites_are_copied_and_values_never_listed(monkeypatch):
    jar = [{"name": "SPC_EC", "value": "secret-1", "domain": ".shopee.tw", "path": "/"},
           {"name": "SID", "value": "secret-2", "domain": ".google.com", "path": "/"},
           {"name": "dev", "value": "x", "domain": "localhost", "path": "/"}]
    calls = []
    async def docker(*args, timeout=45, check=True, input=None):
        calls.append((args, input))
        if args[1] == "-i": return 0, str(len(json.loads(input))), ""
        return 0, json.dumps(jar), ""
    monkeypatch.setattr(logins_module.computers_module, "docker", docker)
    computers = Computers()
    logins = Logins(computers)
    sites = await logins.sites("a")
    assert sites == [{"site": "google.com", "cookies": 1}, {"site": "shopee.tw", "cookies": 1}]
    assert "secret" not in json.dumps(sites)
    done = await logins.copy("a", "cold", ["shopee.tw"])
    assert done == {"copied": 1, "sites": ["shopee.tw"]} and computers.started == ["cold"]
    sent = json.loads(calls[-1][1])
    assert [c["name"] for c in sent] == ["SPC_EC"] and calls[-1][0][2] == "box-cold"
    with pytest.raises(ValueError): await logins.copy("a", "a", ["shopee.tw"])
    with pytest.raises(ValueError): await logins.copy("a", "b", ["example.com"])
