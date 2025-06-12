import("/static/js/asciinema-player.esm.min.js").then((mod) => console.log(mod));
const ensurePlayer = (() => {
    let promise;
    return () => {
        if (promise) return promise;
        promise = Promise.all([
            import("/static/js/asciinema-player.esm.min.js"),

            new Promise((res, rej) => {
                if (document.querySelector('link[href="/static/css/asciinema-player.css"]'))
                    return res();
                const link = Object.assign(document.createElement("link"), {
                    rel: "stylesheet",
                    href: "/static/css/asciinema-player.css",
                });
                link.onload = res;
                link.onerror = rej;
                document.head.appendChild(link);
            }),
        ]).then(([mod]) => mod);

        return promise;
    };
})();

export class PtyPlayer {
    #core;
    #opts;
    #url;
    #mount;
    #state = { isPlaying: false };

    constructor(
        url,
        mount,
        {
            cols = 80,
            rows = 24,
            autoPlay = false,
            preload = false,
            loop = false,
            startAt = 0,
            speed = 1,
            idleTimeLimit = null,
            theme = "auto/asciinema",
            fit = "width",
            controls = "auto",
            markers = [],
            ...rest
        } = {},
    ) {
        this.#url = url;
        this.#mount = mount;
        this.#opts = {
            cols,
            rows,
            autoPlay,
            preload,
            loop,
            startAt,
            speed,
            idleTimeLimit,
            theme,
            fit,
            controls,
            markers,
            rest,
        };
        this.#core = this.#createCore();
    }

    async #createCore() {
        const {
            speed,
            idleTimeLimit,
            markers,
            cols,
            rows,
            autoPlay,
            preload,
            loop,
            startAt,
            theme,
            fit,
            controls,
            rest,
        } = this.#opts;

        const AsciinemaPlayer = await ensurePlayer();
        console.log(AsciinemaPlayer);
        const core = AsciinemaPlayer.create(this.#url, this.#mount, {
            cols,
            rows,
            autoplay: autoPlay,
            preload,
            loop,
            startAt,
            speed,
            idleTimeLimit,
            theme,
            fit,
            controls,
            markers: this.#opts.idleTimeLimit == null ? markers.map((m) => [m.second, m.note]) : [],
            ...rest,
        });

        core.addEventListener("playing", () => (this.#state.isPlaying = true));
        core.addEventListener("pause", () => (this.#state.isPlaying = false));
        core.addEventListener("ended", () => (this.#state.isPlaying = false));

        this.#state.isPlaying = !!autoPlay;

        return core;
    }

    async #recreateSeek() {
        const t = this.#core.getCurrentTime();
        this.#opts.autoPlay = this.#state.isPlaying;
        this.#core.dispose();
        this.#core = this.#createCore();
        this.#core.seek(t);
    }

    async #recreate() {
        this.#opts.autoPlay = false;
        this.#state.isPlaying = false;
        this.#core.dispose();
        this.#core = this.#createCore();
    }

    get isPlaying() {
        return this.#state.isPlaying;
    }
    get speed() {
        return this.#opts.speed;
    }
    get idleTimeLimit() {
        return this.#opts.idleTimeLimit;
    }
    get core() {
        return this.#core;
    }
    get markers() {
        return this.#opts.markers;
    }

    async setSpeed(v) {
        this.#opts.speed = v;
        await this.#recreateSeek();
    }

    async setIdleTimeLimit(v) {
        this.#opts.idleTimeLimit = v;
        await this.#recreate();
    }

    async addMarker(sec, note) {
        this.#opts.markers.push({ second: sec, note });
        await this.#recreateSeek();
    }

    async deleteMarker(id) {
        const idx = this.#opts.markers.findIndex((m) => m.id === id);
        if (idx === -1) return;
        this.#opts.markers.splice(idx, 1);
        await this.#recreateSeek();
    }

    play() {
        this.#core.play();
    }
    pause() {
        this.#core.pause();
    }
    getCurrentTime() {
        return this.#core.getCurrentTime();
    }
    dispose() {
        this.#core.dispose();
    }
}

class ByteReader {
    constructor(buf, offset = 0) {
        this.buf = buf;
        this.pos = offset;
        this.dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    }

    u8() {
        if (this.pos >= this.buf.length) throw new RangeError("EOF");
        return this.buf[this.pos++];
    }

    u16le() {
        if (this.pos + 2 > this.buf.length) throw new RangeError("EOF");
        const val = this.dv.getUint16(this.pos, true);
        this.pos += 2;
        return val;
    }

    f32le() {
        if (this.pos + 4 > this.buf.length) throw new RangeError("EOF");
        const val = this.dv.getFloat32(this.pos, true);
        this.pos += 4;
        return val;
    }

    u128le() {
        if (this.pos + 16 > this.buf.length) throw new RangeError("EOF");

        if (typeof this.dv.getBigUint64 === "function") {
            console.log("!!!");
            const low = this.dv.getBigUint64(this.pos, true);
            const high = this.dv.getBigUint64(this.pos + 8, true);
            this.pos += 16;
            return (high << 64n) | low;
        }

        let res = 0n;
        for (let i = 15; i >= 0; --i) {
            res = (res << 8n) | BigInt(this.buf[this.pos + i]);
        }
        this.pos += 16;
        return res;
    }

    varint() {
        let res = 0,
            shift = 0;
        for (;;) {
            const byte = this.u8();
            res |= (byte & 0x7f) << shift;
            if (byte < 0x80) return res;
            shift += 7;
            if (shift > 53) throw new RangeError("varint too large");
        }
    }

    slice(n) {
        if (this.pos + n > this.buf.length) throw new RangeError("EOF");
        const chunk = this.buf.subarray(this.pos, this.pos + n);
        this.pos += n;
        return chunk;
    }

    skip(n) {
        this.pos += n;
    }

    get remaining() {
        return this.buf.length - this.pos;
    }
}

// export async function parseBinary(response) {
//     const buf = new ByteReader(new Uint8Array(await response.arrayBuffer()));
//     const dec = new TextDecoder("utf-8");

//     const events = [];
//     let last;

//     buf.skip(16); // skip timestamp
//     while (buf.remaining > 0) {
//         const elapsed = buf.f32le();
//         last = elapsed;
//         const kind = buf.u8();
//         if (kind === 0) {
//             // input
//             const len = buf.varint();
//             const bytes = buf.slice(len);
//             const payload = dec.decode(bytes, { stream: true, fatal: true });
//             events.push([elapsed, "i", payload]);
//         } else if (kind === 1) {
//             // output
//             const len = buf.varint();
//             const bytes = buf.slice(len);
//             const payload = dec.decode(bytes, { stream: true, fatal: true });
//             events.push([elapsed, "o", payload]);
//         } else if (kind === 2) {
//             // resize
//             const rows = buf.u16le();
//             const cols = buf.u16le();
//             events.push([elapsed, "r", `${cols}x${rows}`]);
//         } else {
//             throw new RangeError("unknown event");
//         }
//     }

//     return {
//         cols: 80,
//         rows: 24,
//         events: events,
//     };
// }
