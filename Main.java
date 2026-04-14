import java.util.ArrayList;
import java.util.List;

public class Main {

    public static void main(String[] args) {
        List<String> items = new ArrayList<>();
        items.add("Hello");
        items.add("World");

        for (String item : items) {
            System.out.println(item);
        }

        // Try introducing a type error:
        int x = 10;
    }

    public static int add(int a, int b) {
        return a + b;
    }
}