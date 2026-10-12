import asyncio
import threading

from routing.server import Engines, confident_choice
from routing.calibrate import fit


def test_classifier_probability_and_calibration_do_not_fabricate_confidence():
    assert confident_choice({"choice":"answer","answer_confidence":.1,"confidence":.99},.9) is None
    assert confident_choice({"choice":"answer","answer_confidence":.95},.9)==("answer",.95)
    assert confident_choice({"choice":"answer"},.9) is None
    assert confident_choice({"choice":"answer","answer_confidence":float("nan")},.9) is None
    engine=Engines();engine.thresholds={"laya":{"answer":.7,"work":2}}
    assert engine.acceptance_floor("laya","answer",.9)==.9
    assert engine.acceptance_floor("laya","work",.9)==2
    row={"source":"laya","route":"answer","expected":"answer","confidence":.95}
    thresholds,_=fit([row]*20,[{**row,"expected":"native"}]*20)
    assert thresholds["laya"]["answer"]==2


async def test_timed_out_classifier_does_not_start_another_cpu_job():
    engine=Engines();release=threading.Event()
    try:
        result,reason=await engine.run(lambda:release.wait(1),20)
        assert result is None and reason=="timeout" and engine.busy
        assert (await engine.run(lambda:42,20))[1]=="busy"
    finally: release.set()
    for _ in range(100):
        if not engine.busy: break
        await asyncio.sleep(.005)
    assert not engine.busy and await engine.run(lambda:42,100)==(42,None)
