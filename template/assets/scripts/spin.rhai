// Turns its entity around the vertical axis and bobs it up and down.
// Edit this file while the game runs: the change applies at once.

fn init() {
    this.home = this.entity.position;
    this.degrees_per_second = 90.0;
}

fn update(dt) {
    let turn = this.entity.rotation;
    turn.y += this.degrees_per_second * dt;
    this.entity.rotation = turn;

    let height = (elapsed() * 2.0).sin() * 0.2;
    this.entity.position = this.home + vec3(0, height, 0);
}
