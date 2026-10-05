package p;

public class A {
    static class Base { String string; }
    static class Inner extends Base {
        void run(){String string = new String();}
    }
}
