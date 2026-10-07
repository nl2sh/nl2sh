package example;
class Target {
    void touch(String value) {}
    void touch(int value) {}
}
class Base {}
public class Probe extends Base implements Runnable {
    Target field;
    public void run() { work(new Target()); }
    Target work(Target value) {
        value.touch("needle\u0000\ud83d\udc08");
        value.touch(7);
        return value;
    }
    int packed(int value) {
        switch (value) { case 1: return 11; case 2: return 12; case 3: return 13; default: return -1; }
    }
    int sparse(int value) {
        switch (value) { case 1: return 1; case 100: return 2; case 10000: return 3; default: return -1; }
    }
    int[] data() { return new int[] { 1, 2, 3, 4, 5, 6, 7, 8 }; }
}
