// Slides a kinematic body back and forth along X around where it started.

fn init() {
    this.home = this.entity.position;
    this.reach = 3.0;
}

fn fixed_update(dt) {
    let offset = (elapsed() * 0.8).sin() * this.reach;
    this.entity.position = this.home + vec3(offset, 0, 0);
}
