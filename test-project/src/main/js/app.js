class App {
    #name;

    constructor(name) {
        this.#name = name;
    }

    run() {
        console.log(this.#name);
    }
}

function main() {
    const app = new App("codix");
    app.run();
}

const helper = () => {
    return 42;
};
