package p;

import java.io.IOException;

public class A {
    static class Resource implements AutoCloseable {
        public void close() throws Exception {}
        void read() throws IOException {}
    }
    void run() throws IOException {
        try (Resource in = new Resource()) {
            in.read();
        } catch (IOException e) {
            throw e;
        } catch (Exception e) {
            // TODO Auto-generated catch block
            e.printStackTrace();
        }
    }
}
