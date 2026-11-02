"""Tests for guest/varmint-gamepad; run with `python3 -m unittest discover -s tests/guest`."""

import importlib.machinery
import importlib.util
import json
import struct
import unittest
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[2] / "guest" / "varmint-gamepad"
# Shared with the host's tests (src/virtio/console/gamepad_tests.rs).
VECTORS = json.loads((Path(__file__).parent / "gamepad-protocol-vectors.json").read_text())
_loader = importlib.machinery.SourceFileLoader("varmint_gamepad", str(SOURCE))
_spec = importlib.util.spec_from_loader("varmint_gamepad", _loader)
gp = importlib.util.module_from_spec(_spec)
_loader.exec_module(gp)
gp.log = lambda message: None


def attach(id, name="Pad"):
    raw = name.encode()
    return gp.encode(gp.MSG_ATTACH, struct.pack("<IB", id, len(raw)) + raw)


def detach(id):
    return gp.encode(gp.MSG_DETACH, struct.pack("<I", id))


def state(id, buttons=0, lx=0, ly=0, rx=0, ry=0, lt=0, rt=0):
    return gp.encode(gp.MSG_STATE, struct.pack("<IIhhhhHH", id, buttons, lx, ly, rx, ry, lt, rt))


def host_hello(version=gp.PROTOCOL_VERSION):
    return gp.encode(gp.MSG_HELLO, struct.pack("<H", version))


class FakeDevice:
    def __init__(self, id, log):
        self.id = id
        self.log = log
        self.fd = -1
        self.closed = False
        self.requests = []
        log.append(("create", id))

    def write(self, events):
        self.log.append(("write", self.id, dict(events)))

    def read_requests(self):
        requests, self.requests = self.requests, []
        return requests

    def serve_upload(self, request_id, rumble, now):
        id, effect = self.uploads[request_id]
        rumble.upload(id, effect, now)

    def serve_erase(self, request_id, rumble):
        rumble.erase(self.erases[request_id])

    def close(self):
        self.closed = True
        self.log.append(("close", self.id))


class Clock:
    def __init__(self):
        self.ms = 0

    def __call__(self):
        return self.ms / 1000


def make_agent():
    log, sent, devices, clock = [], [], {}, Clock()

    def make_device(id):
        device = FakeDevice(id, log)
        devices[id] = device
        return device

    agent = gp.Agent(sent.append, make_device=make_device, clock=clock)
    return agent, log, sent, devices, clock


def written(log):
    return [entry[2] for entry in log if entry[0] == "write"]


class IoctlNumbers(unittest.TestCase):
    def test_match_the_arm64_kernel_headers(self):
        self.assertEqual(gp.UI_DEV_CREATE, 0x5501)
        self.assertEqual(gp.UI_DEV_DESTROY, 0x5502)
        self.assertEqual(gp.UI_DEV_SETUP, 0x405C5503)
        self.assertEqual(gp.UI_ABS_SETUP, 0x401C5504)
        self.assertEqual(gp.UI_SET_EVBIT, 0x40045564)
        self.assertEqual(gp.UI_SET_KEYBIT, 0x40045565)
        self.assertEqual(gp.UI_SET_ABSBIT, 0x40045567)
        self.assertEqual(gp.UI_SET_FFBIT, 0x4004556B)
        self.assertEqual(gp.UI_SET_PHYS, 0x4008556C)
        self.assertEqual(gp.UI_BEGIN_FF_UPLOAD, 0xC06855C8)
        self.assertEqual(gp.UI_END_FF_UPLOAD, 0x406855C9)
        self.assertEqual(gp.UI_BEGIN_FF_ERASE, 0xC00C55CA)
        self.assertEqual(gp.UI_END_FF_ERASE, 0x400C55CB)
        self.assertEqual(gp.INPUT_EVENT.size, 24)


class Mapping(unittest.TestCase):
    def values(self, **fields):
        return gp.linux_values(gp.State(1, **fields))

    def test_rest_is_all_released_and_centered(self):
        self.assertTrue(all(value == 0 for value in gp.REST.values()))

    def test_face_buttons_follow_xpad_codes_by_position(self):
        cases = {
            gp.SOUTH: gp.BTN_A,
            gp.EAST: gp.BTN_B,
            gp.WEST: gp.BTN_X,  # 0x133, BTN_NORTH's code, as xpad reports Xbox X
            gp.NORTH: gp.BTN_Y,  # 0x134, BTN_WEST's code
        }
        for bit, code in cases.items():
            values = self.values(buttons=bit)
            pressed = [c for (kind, c), v in values.items() if kind == gp.EV_KEY and v]
            self.assertEqual(pressed, [code])
        self.assertEqual(gp.BTN_X, 0x133)
        self.assertEqual(gp.BTN_Y, 0x134)

    def test_every_other_button(self):
        cases = {
            gp.LEFT_SHOULDER: gp.BTN_TL,
            gp.RIGHT_SHOULDER: gp.BTN_TR,
            gp.OPTIONS: gp.BTN_SELECT,
            gp.MENU: gp.BTN_START,
            gp.HOME: gp.BTN_MODE,
            gp.LEFT_STICK: gp.BTN_THUMBL,
            gp.RIGHT_STICK: gp.BTN_THUMBR,
        }
        for bit, code in cases.items():
            self.assertEqual(self.values(buttons=bit)[(gp.EV_KEY, code)], 1)
        all_bits = sum(bit for bit, _ in gp.BUTTONS)
        self.assertTrue(all(self.values(buttons=all_bits)[(gp.EV_KEY, code)] for _, code in gp.BUTTONS))

    def test_sticks_center_extrema_and_y_up_is_negative(self):
        center = self.values()
        for code in (gp.ABS_X, gp.ABS_Y, gp.ABS_RX, gp.ABS_RY):
            self.assertEqual(center[(gp.EV_ABS, code)], 0)
        right_up = self.values(left_x=32767, left_y=32767, right_x=32767, right_y=32767)
        self.assertEqual(right_up[(gp.EV_ABS, gp.ABS_X)], 32767)
        self.assertEqual(right_up[(gp.EV_ABS, gp.ABS_Y)], -32767)
        self.assertEqual(right_up[(gp.EV_ABS, gp.ABS_RX)], 32767)
        self.assertEqual(right_up[(gp.EV_ABS, gp.ABS_RY)], -32767)
        left_down = self.values(left_x=-32767, left_y=-32767, right_x=-32767, right_y=-32767)
        self.assertEqual(left_down[(gp.EV_ABS, gp.ABS_X)], -32767)
        self.assertEqual(left_down[(gp.EV_ABS, gp.ABS_Y)], 32767)
        self.assertEqual(left_down[(gp.EV_ABS, gp.ABS_RY)], 32767)
        self.assertEqual(self.values(left_y=-32768)[(gp.EV_ABS, gp.ABS_Y)], 32767, "clamped to the axis range")

    def test_stick_values_stay_within_the_advertised_ranges(self):
        ranges = {code: (low, high) for code, low, high, _, _ in gp.AXES}
        for value in (-32768, -32767, -1, 0, 1, 32767):
            for (kind, code), out in self.values(left_x=value, left_y=value, right_x=value, right_y=value).items():
                if kind == gp.EV_ABS:
                    low, high = ranges[code]
                    self.assertTrue(low <= out <= high, (code, value, out))

    def test_triggers_are_analog_over_xpad_range(self):
        self.assertEqual(self.values(left_trigger=0)[(gp.EV_ABS, gp.ABS_Z)], 0)
        self.assertEqual(self.values(left_trigger=65535)[(gp.EV_ABS, gp.ABS_Z)], 255)
        self.assertEqual(self.values(right_trigger=65535)[(gp.EV_ABS, gp.ABS_RZ)], 255)
        self.assertEqual(self.values(left_trigger=32768)[(gp.EV_ABS, gp.ABS_Z)], 128)
        self.assertEqual(self.values(right_trigger=257)[(gp.EV_ABS, gp.ABS_RZ)], 1)

    def test_dpad_is_a_hat(self):
        hat = lambda buttons: (  # noqa: E731
            self.values(buttons=buttons)[(gp.EV_ABS, gp.ABS_HAT0X)],
            self.values(buttons=buttons)[(gp.EV_ABS, gp.ABS_HAT0Y)],
        )
        self.assertEqual(hat(0), (0, 0))
        self.assertEqual(hat(gp.DPAD_UP), (0, -1))
        self.assertEqual(hat(gp.DPAD_DOWN), (0, 1))
        self.assertEqual(hat(gp.DPAD_LEFT), (-1, 0))
        self.assertEqual(hat(gp.DPAD_RIGHT), (1, 0))
        self.assertEqual(hat(gp.DPAD_UP | gp.DPAD_RIGHT), (1, -1))
        self.assertEqual(hat(gp.DPAD_LEFT | gp.DPAD_RIGHT), (0, 0))

    def test_the_device_advertises_what_the_mapping_produces(self):
        advertised = {(gp.EV_KEY, code) for _, code in gp.BUTTONS} | {(gp.EV_ABS, a[0]) for a in gp.AXES}
        self.assertEqual(set(gp.REST), advertised)


class SharedVectors(unittest.TestCase):
    def test_host_messages_decode(self):
        for vector in VECTORS["host_to_guest"]:
            with self.subTest(vector=vector):
                (message,) = gp.take_messages(bytearray(bytes.fromhex(vector["bytes"])))
                kind = vector["message"]
                if kind == "hello":
                    self.assertIsInstance(message, gp.Hello)
                    self.assertEqual(message.version, vector["version"])
                elif kind == "hid_attach":
                    self.assertIsInstance(message, gp.HidAttach)
                    for field in ("id", "bus", "vendor", "product", "version", "country", "name"):
                        self.assertEqual(getattr(message, field), vector[field], field)
                    self.assertEqual(message.descriptor.hex(), vector["descriptor"])
                elif kind == "hid_input":
                    self.assertIsInstance(message, gp.HidInput)
                    self.assertEqual((message.id, message.report.hex()), (vector["id"], vector["report"]))
                elif kind == "hid_reply":
                    self.assertIsInstance(message, gp.HidReply)
                    self.assertEqual(
                        (message.id, message.request, message.errno, message.data.hex()),
                        (vector["id"], vector["request"], vector["errno"], vector["data"]),
                    )
                elif kind == "attach":
                    self.assertIsInstance(message, gp.Attach)
                    self.assertEqual((message.id, message.name), (vector["id"], vector["name"]))
                elif kind == "detach":
                    self.assertIsInstance(message, gp.Detach)
                    self.assertEqual(message.id, vector["id"])
                else:
                    self.assertIsInstance(message, gp.State)
                    buttons = sum(getattr(gp, name.upper()) for name in vector["buttons"])
                    fields = ("id", "left_x", "left_y", "right_x", "right_y", "left_trigger", "right_trigger")
                    self.assertEqual(message.buttons, buttons)
                    for field in fields:
                        self.assertEqual(getattr(message, field), vector[field], field)

    def test_guest_messages_encode(self):
        for vector in VECTORS["guest_to_host"]:
            with self.subTest(vector=vector):
                kind = vector["message"]
                if kind == "hello" and "features" not in vector:
                    continue  # an older guest's HELLO, which only the host has to accept
                if kind == "hello":
                    self.assertEqual(vector["version"], gp.PROTOCOL_VERSION)
                    encoded = gp.encode_hello(vector["features"])
                elif kind == "rumble":
                    encoded = gp.encode_rumble(vector["id"], vector["strong"], vector["weak"])
                elif kind == "hid_output":
                    encoded = gp.encode_hid_output(vector["id"], vector["type"], bytes.fromhex(vector["report"]))
                elif kind == "hid_get":
                    encoded = gp.encode_hid_get(vector["id"], vector["request"], vector["type"], vector["report_id"])
                else:
                    encoded = gp.encode_hid_set(
                        vector["id"], vector["request"], vector["type"], bytes.fromhex(vector["report"])
                    )
                self.assertEqual(encoded.hex(), vector["bytes"])


class Protocol(unittest.TestCase):
    def test_decodes_every_host_message(self):
        buf = bytearray(host_hello() + attach(7, "DualSense") + state(7, 5, 1, -2, 3, -4, 5, 6) + detach(7))
        hello, att, st, det = gp.take_messages(buf)
        self.assertEqual(hello.version, gp.PROTOCOL_VERSION)
        self.assertEqual((att.id, att.name), (7, "DualSense"))
        self.assertEqual(
            (st.id, st.buttons, st.left_x, st.left_y, st.right_x, st.right_y, st.left_trigger, st.right_trigger),
            (7, 5, 1, -2, 3, -4, 5, 6),
        )
        self.assertEqual(det.id, 7)
        self.assertEqual(buf, bytearray())

    def test_waits_for_partial_messages(self):
        data = state(1, 1)
        buf = bytearray(data[:5])
        self.assertEqual(gp.take_messages(buf), [])
        buf.extend(data[5:])
        self.assertEqual(len(gp.take_messages(buf)), 1)

    def test_skips_unknown_and_short_messages_and_ignores_extra_fields(self):
        buf = bytearray(
            gp.encode(0x7F, b"future")
            + gp.encode(gp.MSG_STATE, b"\x01\x00")
            + gp.encode(gp.MSG_ATTACH, struct.pack("<IB", 1, 10) + b"abc")
            + gp.encode(gp.MSG_DETACH, struct.pack("<I", 3) + b"later fields")
        )
        (message,) = gp.take_messages(buf)
        self.assertIsInstance(message, gp.Detach)
        self.assertEqual(message.id, 3)

    def test_rejects_corrupt_lengths(self):
        with self.assertRaises(gp.ProtocolError):
            gp.take_messages(bytearray(b"\x00\x00\x01"))
        with self.assertRaises(gp.ProtocolError):
            gp.take_messages(bytearray(struct.pack("<H", gp.MAX_MESSAGE_LEN + 1)))

    def test_encodes_guest_messages(self):
        self.assertEqual(gp.encode_hello(0), b"\x07\x00\x01\x01\x00\x00\x00\x00\x00")
        self.assertEqual(gp.encode_rumble(2, 0xFFFF, 1), b"\x09\x00\x05\x02\x00\x00\x00\xff\xff\x01\x00")


class Coalescing(unittest.TestCase):
    def ids_and_x(self, messages):
        return [(type(m).__name__, getattr(m, "id", None), getattr(m, "left_x", None)) for m in messages]

    def test_drops_stale_analog_states_but_keeps_button_transitions(self):
        messages = gp.take_messages(
            bytearray(
                state(1, 0, lx=1)
                + state(1, 0, lx=2)
                + state(1, gp.SOUTH, lx=3)
                + state(1, 0, lx=4)
                + state(1, 0, lx=5)
                + state(2, 0, lx=9)
            )
        )
        self.assertEqual(
            self.ids_and_x(gp.coalesce(messages)),
            [("State", 1, 2), ("State", 1, 3), ("State", 1, 5), ("State", 2, 9)],
        )

    def test_lifecycle_messages_separate_runs(self):
        messages = gp.take_messages(bytearray(state(1, lx=1) + detach(1) + attach(1) + state(1, lx=2)))
        self.assertEqual(
            self.ids_and_x(gp.coalesce(messages)),
            [("State", 1, 1), ("Detach", 1, None), ("Attach", 1, None), ("State", 1, 2)],
        )


class Lifecycle(unittest.TestCase):
    def test_says_hello_first(self):
        agent, _, sent, _, _ = make_agent()
        agent.start()
        self.assertEqual(sent, [gp.encode_hello(gp.FEATURE_RAW_HID)])

    def test_hello_offers_raw_hid_only_with_uhid(self):
        sent = []
        gp.Agent(sent.append, raw_hid=False).start()
        self.assertEqual(sent, [gp.encode_hello(0)])

    def test_attach_state_detach(self):
        agent, log, _, devices, _ = make_agent()
        agent.receive(host_hello() + attach(1) + state(1, gp.SOUTH | gp.DPAD_UP, lx=100, lt=65535))
        self.assertEqual(log[0], ("create", 1))
        self.assertEqual(
            written(log),
            [
                {
                    (gp.EV_KEY, gp.BTN_A): 1,
                    (gp.EV_ABS, gp.ABS_X): 100,
                    (gp.EV_ABS, gp.ABS_Z): 255,
                    (gp.EV_ABS, gp.ABS_HAT0Y): -1,
                }
            ],
        )
        agent.receive(state(1, gp.SOUTH | gp.DPAD_UP, lx=100, lt=65535))
        self.assertEqual(len(written(log)), 1, "an unchanged state writes nothing")

        agent.receive(detach(1))
        self.assertTrue(devices[1].closed)
        self.assertNotIn(1, agent.pads)

    def test_held_buttons_and_axes_are_released_before_removal(self):
        agent, log, _, _, _ = make_agent()
        agent.receive(attach(1) + state(1, gp.HOME | gp.RIGHT_SHOULDER, ry=-32767, rt=65535))
        agent.receive(detach(1))
        self.assertEqual(
            written(log)[-1],
            {
                (gp.EV_KEY, gp.BTN_MODE): 0,
                (gp.EV_KEY, gp.BTN_TR): 0,
                (gp.EV_ABS, gp.ABS_RY): 0,
                (gp.EV_ABS, gp.ABS_RZ): 0,
            },
        )
        self.assertEqual(log[-1], ("close", 1))

    def test_reattach_replaces_the_device(self):
        agent, log, _, devices, _ = make_agent()
        agent.receive(attach(1))
        first = devices[1]
        agent.receive(attach(1, "Again"))
        self.assertTrue(first.closed)
        self.assertIsNot(devices[1], first)
        self.assertEqual(agent.pads[1].name, "Again")
        self.assertEqual(len(agent.pads), 1)

    def test_multiple_controllers_are_independent(self):
        agent, log, _, devices, _ = make_agent()
        agent.receive(attach(1) + attach(2) + state(2, gp.EAST) + detach(1))
        self.assertEqual(set(agent.pads), {2})
        self.assertTrue(devices[1].closed)
        self.assertFalse(devices[2].closed)
        self.assertIn(("write", 2, {(gp.EV_KEY, gp.BTN_B): 1}), log)

    def test_state_for_unknown_controller_is_ignored(self):
        agent, log, _, _, _ = make_agent()
        agent.receive(state(5, gp.SOUTH))
        self.assertEqual(log, [])

    def test_device_creation_failure_is_survivable(self):
        sent = []

        def fail(_):
            raise PermissionError("denied")

        agent = gp.Agent(sent.append, make_device=fail, clock=Clock())
        agent.receive(attach(1) + state(1, gp.SOUTH))
        self.assertEqual(agent.pads, {})

    def test_remove_all_releases_everything(self):
        agent, _, _, devices, _ = make_agent()
        agent.receive(attach(1) + attach(2))
        agent.remove_all()
        self.assertTrue(devices[1].closed and devices[2].closed)
        self.assertEqual(agent.pads, {})


class Rumble(unittest.TestCase):
    def fx(self, strong, weak, length, delay):
        return gp.Effect(strong, weak, length, delay)

    def test_plays_after_delay_for_length(self):
        r = gp.Rumble()
        r.upload(0, self.fx(1000, 2000, 100, 50), 0)
        self.assertEqual(r.level(0), (0, 0), "uploaded only")
        r.play(0, 1, 10)
        self.assertEqual(r.level(59), (0, 0))
        self.assertEqual(r.next_change(59), 60)
        self.assertEqual(r.level(60), (1000, 2000))
        self.assertEqual(r.next_change(60), 160)
        self.assertEqual(r.level(159), (1000, 2000))
        self.assertEqual(r.level(160), (0, 0))
        self.assertIsNone(r.next_change(160))

    def test_zero_length_plays_until_stopped(self):
        r = gp.Rumble()
        r.upload(3, self.fx(500, 0, 0, 0), 0)
        r.play(3, 1, 0)
        self.assertEqual(r.level(1_000_000), (500, 0))
        self.assertIsNone(r.next_change(1_000_000))
        r.play(3, 0, 1_000_001)
        self.assertEqual(r.level(1_000_001), (0, 0))

    def test_repeats_with_delay_before_each_play(self):
        r = gp.Rumble()
        r.upload(1, self.fx(100, 100, 10, 5), 0)
        r.play(1, 2, 0)
        self.assertEqual(r.level(5), (100, 100))
        self.assertEqual(r.level(15), (0, 0))
        self.assertEqual(r.next_change(15), 20)
        self.assertEqual(r.level(20), (100, 100))
        self.assertEqual(r.level(30), (0, 0))
        self.assertIsNone(r.next_change(30))

    def test_concurrent_effects_add_up_and_saturate(self):
        r = gp.Rumble()
        r.upload(0, self.fx(40000, 1000, 0, 0), 0)
        r.upload(1, self.fx(40000, 2000, 50, 0), 0)
        r.play(0, 1, 0)
        r.play(1, 1, 0)
        self.assertEqual(r.level(10), (0xFFFF, 3000))
        self.assertEqual(r.level(50), (40000, 1000))

    def test_reupload_restarts_a_playing_effect(self):
        r = gp.Rumble()
        r.upload(0, self.fx(1000, 0, 100, 0), 0)
        r.play(0, 1, 0)
        r.upload(0, self.fx(3000, 0, 100, 0), 80)
        self.assertEqual(r.level(80), (3000, 0))
        self.assertEqual(r.level(150), (3000, 0), "the new parameters run their full length")
        self.assertEqual(r.level(180), (0, 0))
        r.upload(0, self.fx(5000, 0, 100, 0), 200)
        self.assertEqual(r.level(200), (0, 0), "an idle effect stays idle when updated")

    def test_stop_and_erase(self):
        r = gp.Rumble()
        r.upload(0, self.fx(1000, 0, 0, 0), 0)
        r.upload(1, self.fx(2000, 0, 0, 0), 0)
        r.play(0, 1, 0)
        r.play(1, 1, 0)
        r.play(0, 0, 10)
        self.assertEqual(r.level(10), (2000, 0))
        r.erase(1)
        self.assertEqual(r.level(10), (0, 0))
        r.play(1, 1, 20)
        self.assertEqual(r.level(20), (0, 0), "an erased effect cannot be played")


class ForceFeedbackPath(unittest.TestCase):
    def test_parses_a_rumble_ff_effect(self):
        buf = bytearray(gp.FF_UPLOAD_SIZE)
        struct.pack_into("<HhHHHHH", buf, gp.FF_EFFECT_OFFSET, gp.FF_RUMBLE, 4, 0, 0, 0, 250, 20)
        struct.pack_into("<HH", buf, gp.FF_EFFECT_OFFSET + 16, 0xC000, 0x4000)
        kind, id, effect = gp.parse_ff_effect(buf, gp.FF_EFFECT_OFFSET)
        self.assertEqual((kind, id), (gp.FF_RUMBLE, 4))
        self.assertEqual((effect.strong, effect.weak, effect.length, effect.delay), (0xC000, 0x4000, 250, 20))

    def test_other_effect_types_are_refused(self):
        buf = bytearray(gp.FF_UPLOAD_SIZE)
        struct.pack_into("<H", buf, gp.FF_EFFECT_OFFSET, 0x51)  # FF_PERIODIC
        self.assertIsNone(gp.parse_ff_effect(buf, gp.FF_EFFECT_OFFSET)[2])

    def test_upload_play_stop_erase_reach_the_host(self):
        agent, _, sent, devices, clock = make_agent()
        agent.receive(attach(1))
        device = devices[1]
        device.uploads = {7: (0, self.effect(0x8000, 0x2000, 0, 0)), 8: (0, self.effect(0xFFFF, 0, 0, 0))}
        device.erases = {9: 0}

        device.requests = [(gp.EV_UINPUT, gp.UI_FF_UPLOAD, 7), (gp.EV_FF, 0, 1)]
        agent.serve_device(agent.pads[1])
        self.assertIsNone(agent.update_rumble())
        self.assertEqual(sent[-1], gp.encode_rumble(1, 0x8000, 0x2000))

        clock.ms = 10
        device.requests = [(gp.EV_UINPUT, gp.UI_FF_UPLOAD, 8)]
        agent.serve_device(agent.pads[1])
        agent.update_rumble()
        self.assertEqual(sent[-1], gp.encode_rumble(1, 0xFFFF, 0), "re-upload replaces a playing effect")

        device.requests = [(gp.EV_FF, 0, 0)]
        agent.serve_device(agent.pads[1])
        agent.update_rumble()
        self.assertEqual(sent[-1], gp.encode_rumble(1, 0, 0))

        count = len(sent)
        device.requests = [(gp.EV_UINPUT, gp.UI_FF_ERASE, 9), (gp.EV_FF, 0, 1)]
        agent.serve_device(agent.pads[1])
        agent.update_rumble()
        self.assertEqual(len(sent), count, "an erased effect does not play")

    def test_timed_effect_reports_when_to_wake(self):
        agent, _, sent, devices, clock = make_agent()
        agent.receive(attach(1))
        devices[1].uploads = {1: (0, self.effect(100, 200, 300, 50))}
        devices[1].requests = [(gp.EV_UINPUT, gp.UI_FF_UPLOAD, 1), (gp.EV_FF, 0, 1)]
        agent.serve_device(agent.pads[1])
        self.assertEqual(agent.update_rumble(), 50)
        clock.ms = 50
        self.assertEqual(agent.update_rumble(), 300)
        self.assertEqual(sent[-1], gp.encode_rumble(1, 100, 200))
        clock.ms = 350
        self.assertIsNone(agent.update_rumble())
        self.assertEqual(sent[-1], gp.encode_rumble(1, 0, 0))

    def effect(self, strong, weak, length, delay):
        return gp.Effect(strong, weak, length, delay)


if __name__ == "__main__":
    unittest.main()


def hid_attach(id, descriptor=b"\x05\x01\x09\x05"):
    name = b"DualSense Wireless Controller"
    payload = struct.pack("<IHHHHIB", id, 5, 0x054C, 0x0CE6, 0x0100, 0, len(name)) + name
    return gp.encode(gp.MSG_HID_ATTACH, payload + struct.pack("<H", len(descriptor)) + descriptor)


def hid_input(id, report):
    return gp.encode(gp.MSG_HID_INPUT, struct.pack("<IH", id, len(report)) + report)


def hid_reply(id, request, error, data=b""):
    return gp.encode(gp.MSG_HID_REPLY, struct.pack("<IIHH", id, request, error, len(data)) + data)


class FakeUhid:
    def __init__(self, attach, log):
        self.attach = attach
        self.log = log
        self.fd = -2
        self.requests = []
        self.closed = False
        log.append(("create", attach.id, attach.name, attach.descriptor))

    def write(self, event):
        self.log.append(("write", event))

    def read_requests(self):
        requests, self.requests = self.requests, []
        return requests

    def close(self):
        self.closed = True
        self.log.append(("close", self.attach.id))


def make_hid_agent():
    log, sent, devices = [], [], {}

    def make_hid_device(attach):
        devices[attach.id] = FakeUhid(attach, log)
        return devices[attach.id]

    agent = gp.Agent(sent.append, make_hid_device=make_hid_device, clock=Clock())
    return agent, log, sent, devices


class Uhid(unittest.TestCase):
    def test_create2_puts_fields_at_the_kernel_offsets(self):
        attach = gp.take_messages(bytearray(hid_attach(2)))[0]
        event = gp.uhid_create2(attach, "varmint-gamepad/2")
        self.assertEqual(len(event), gp.UHID_EVENT_SIZE)
        self.assertEqual(struct.unpack_from("<I", event)[0], gp.UHID_CREATE2)
        self.assertEqual(event[4:33], b"DualSense Wireless Controller")
        self.assertEqual(event[33], 0)
        self.assertEqual(event[132:149], b"varmint-gamepad/2")
        self.assertEqual(event[196:260], bytes(64), "uniq stays empty")
        self.assertEqual(struct.unpack_from("<HHIIII", event, 260), (4, 5, 0x054C, 0x0CE6, 0x0100, 0))
        self.assertEqual(event[280:284], b"\x05\x01\x09\x05")

    def test_long_names_keep_their_terminator(self):
        attach = gp.HidAttach(1, 3, 1, 2, 0, 0, "n" * 200, b"\x00")
        event = gp.uhid_create2(attach, "p" * 100)
        self.assertEqual(event[4 + 127], 0)
        self.assertEqual(event[132 + 63], 0)

    def test_input_and_replies(self):
        self.assertEqual(gp.uhid_input2(b"\x31\x80"), struct.pack("<IH", gp.UHID_INPUT2, 2) + b"\x31\x80")
        self.assertEqual(
            gp.uhid_get_report_reply(3, 0, b"\x09\x01"),
            struct.pack("<IIHH", gp.UHID_GET_REPORT_REPLY, 3, 0, 2) + b"\x09\x01",
        )
        self.assertEqual(gp.uhid_set_report_reply(4, 5), struct.pack("<IIH", gp.UHID_SET_REPORT_REPLY, 4, 5))

    def test_decodes_output_and_report_requests(self):
        event = bytearray(gp.UHID_EVENT_SIZE)
        struct.pack_into("<I", event, 0, gp.UHID_OUTPUT)
        event[4:7] = b"\x31\x02\xff"
        struct.pack_into("<HB", event, 4 + gp.UHID_DATA_MAX, 3, 1)
        self.assertEqual(gp.uhid_decode(bytes(event)), (gp.UHID_OUTPUT, (1, b"\x31\x02\xff")))

        event = bytearray(gp.UHID_EVENT_SIZE)
        struct.pack_into("<IIBB", event, 0, gp.UHID_GET_REPORT, 7, 0x05, 0)
        self.assertEqual(gp.uhid_decode(bytes(event)), (gp.UHID_GET_REPORT, (7, 0, 0x05)))

        event = bytearray(gp.UHID_EVENT_SIZE)
        struct.pack_into("<IIBBH", event, 0, gp.UHID_SET_REPORT, 9, 0x80, 0, 2)
        event[12:14] = b"\x80\x01"
        self.assertEqual(gp.uhid_decode(bytes(event)), (gp.UHID_SET_REPORT, (9, 0, b"\x80\x01")))

    def test_ignores_short_reads_and_other_events(self):
        self.assertIsNone(gp.uhid_decode(b"\x00" * 8))
        event = bytearray(gp.UHID_EVENT_SIZE)
        struct.pack_into("<I", event, 0, gp.UHID_START)
        self.assertIsNone(gp.uhid_decode(bytes(event)))


class HidLifecycle(unittest.TestCase):
    def test_attach_input_detach(self):
        agent, log, _, devices = make_hid_agent()
        agent.receive(hid_attach(2) + hid_input(2, b"\x31\x01") + hid_input(2, b"\x31\x02"))
        self.assertEqual(log[0], ("create", 2, "DualSense Wireless Controller", b"\x05\x01\x09\x05"))
        self.assertEqual(log[1:], [("write", gp.uhid_input2(b"\x31\x02"))], "the newest report wins")
        agent.receive(gp.encode(gp.MSG_DETACH, struct.pack("<I", 2)))
        self.assertTrue(devices[2].closed)
        self.assertEqual(agent.hid_pads, {})

    def test_requests_go_to_the_host_and_replies_back_to_the_driver(self):
        agent, log, sent, devices = make_hid_agent()
        agent.receive(hid_attach(2))
        devices[2].requests = [
            (gp.UHID_GET_REPORT, (7, 0, 0x05)),
            (gp.UHID_SET_REPORT, (8, 0, b"\x80\x01")),
            (gp.UHID_OUTPUT, (1, b"\x31\x02")),
        ]
        agent.serve_hid_device(agent.hid_pads[2])
        self.assertEqual(
            sent,
            [
                gp.encode_hid_get(2, 7, 0, 0x05),
                gp.encode_hid_set(2, 8, 0, b"\x80\x01"),
                gp.encode_hid_output(2, 1, b"\x31\x02"),
            ],
        )
        agent.receive(hid_reply(2, 7, 0, b"\x05\xaa") + hid_reply(2, 8, 5) + hid_reply(2, 99, 0))
        self.assertEqual(
            log[1:],
            [("write", gp.uhid_get_report_reply(7, 0, b"\x05\xaa")), ("write", gp.uhid_set_report_reply(8, 5))],
            "a reply nobody waits for is dropped",
        )

    def test_reattach_replaces_and_remove_all_closes(self):
        agent, _, _, devices = make_hid_agent()
        agent.receive(hid_attach(2))
        first = devices[2]
        agent.receive(hid_attach(2))
        self.assertTrue(first.closed)
        agent.remove_all()
        self.assertTrue(devices[2].closed)

    def test_input_for_unknown_devices_is_ignored(self):
        agent, log, _, _ = make_hid_agent()
        agent.receive(hid_input(5, b"\x31"))
        self.assertEqual(log, [])

    def test_coalescing_keeps_order_across_kinds(self):
        messages = gp.take_messages(
            bytearray(hid_input(2, b"\x01") + hid_reply(2, 1, 0) + hid_input(2, b"\x02") + hid_input(2, b"\x03"))
        )
        kept = gp.coalesce(messages)
        self.assertEqual(
            [(type(m).__name__, getattr(m, "report", b"").hex()) for m in kept],
            [("HidInput", "01"), ("HidReply", ""), ("HidInput", "03")],
        )
