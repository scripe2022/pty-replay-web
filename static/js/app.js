const AsciinemaPlayer = window.AsciinemaPlayer;

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

    #createCore() {
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
        const t = await this.#core.getCurrentTime();
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
